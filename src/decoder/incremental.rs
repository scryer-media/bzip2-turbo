//! Slice-driven use of the resumable block parser.
//! Translation of BZip2Decoder.cpp from 7-Zip, LGPL-2.1-or-later.
use super::block::{BlockBody, BlockHeader, BlockInput};
use super::{
    BLOCK_MAGIC, BlockOutput, Huffman, Links, MAX_CODE_BITS, Mode, ParserWorkspace,
    STREAM_END_MAGIC, invalid,
};
use std::io;

#[derive(Clone, Copy)]
enum Stage {
    Header(usize),
    MagicHigh,
    MagicLow(u32),
    Footer,
    BlockHeader,
    Symbols,
    Output,
    Done,
}

// The high-aligned bits carry across input slices. Symbol lookahead may
// temporarily borrow whole bytes beyond the end of a block.
struct SliceBits<'a> {
    input: &'a [u8],
    unread: &'a [u8],
    bits: u64,
    count: u32,
}

impl SliceBits<'_> {
    #[inline]
    fn take(&mut self, n: u32) -> Option<u32> {
        while self.count < n {
            let (&byte, unread) = self.unread.split_first()?;
            self.unread = unread;
            self.bits |= u64::from(byte) << (56 - self.count);
            self.count += 8;
        }
        let value = (self.bits >> (64 - n)) as u32;
        self.bits <<= n;
        self.count -= n;
        Some(value)
    }

    // Before switching back to structural fields or yielding output, return
    // unread lookahead bytes to the caller. Any partial byte stays committed.
    fn release_lookahead(&mut self) {
        let consumed = self.input.len() - self.unread.len();
        let read_ahead = (self.count / 8) as usize;
        debug_assert!(read_ahead <= consumed);
        self.unread = &self.input[consumed - read_ahead..];
        self.count %= 8;
        self.bits &= u64::MAX.checked_shl(64 - self.count).unwrap_or(0);
    }

    #[cold]
    fn tail_symbol(&mut self, table: &Huffman) -> io::Result<Option<usize>> {
        while self.count < MAX_CODE_BITS {
            let Some((&byte, unread)) = self.unread.split_first() else {
                break;
            };
            self.unread = unread;
            self.bits |= u64::from(byte) << (56 - self.count);
            self.count += 8;
        }
        self.lookup(table)
    }

    #[inline(always)]
    fn lookup(&mut self, table: &Huffman) -> io::Result<Option<usize>> {
        let window = (self.bits >> (64 - MAX_CODE_BITS)) as u32;
        match table.lookup(window) {
            // A window padded with zeros resolves a definite symbol only
            // when the entire code is already available.
            Some((symbol, len)) if len <= self.count => {
                self.bits <<= len;
                self.count -= len;
                Ok(Some(symbol))
            }
            None if self.count >= MAX_CODE_BITS => {
                self.bits <<= MAX_CODE_BITS;
                self.count -= MAX_CODE_BITS;
                Err(invalid("invalid Huffman code"))
            }
            _ => Ok(None),
        }
    }
}

impl BlockInput for SliceBits<'_> {
    #[inline]
    fn take(&mut self, n: u32) -> io::Result<Option<u32>> {
        Ok(self.take(n))
    }

    #[inline(always)]
    fn symbol<const BYTEWISE: bool>(&mut self, table: &Huffman) -> io::Result<Option<usize>> {
        if self.count < MAX_CODE_BITS {
            let Some((word, unread)) = self.unread.split_first_chunk::<4>() else {
                return self.tail_symbol(table);
            };
            self.unread = unread;
            self.bits |= u64::from(u32::from_be_bytes(*word)) << (32 - self.count);
            self.count += 32;
        }
        self.lookup(table)
    }
}

pub(crate) struct Incremental {
    stage: Stage,
    bits: u64,
    count: u32,
    limit: usize,
    combined: u32,
    workspace: ParserWorkspace,
    header: BlockHeader,
    body: Option<BlockBody>,
    column: Vec<u8>,
    mode: Mode,
    active: Option<BlockOutput>,
    delivered: u64,
}

impl Incremental {
    pub(crate) fn new(small: bool) -> Self {
        Self {
            stage: Stage::Header(0),
            bits: 0,
            count: 0,
            limit: 0,
            combined: 0,
            workspace: ParserWorkspace::default(),
            header: BlockHeader::default(),
            body: None,
            column: Vec::new(),
            mode: if small {
                Mode::Compact
            } else {
                Mode::platform()
            },
            active: None,
            delivered: 0,
        }
    }

    pub(crate) fn delivered(&self) -> u64 {
        self.delivered
    }

    pub(crate) fn done(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }

    pub(crate) fn process(&mut self, input: &mut &[u8], output: &mut [u8]) -> io::Result<usize> {
        let mut bits = SliceBits {
            input,
            unread: input,
            bits: self.bits,
            count: self.count,
        };
        let result = self.advance(&mut bits, output);
        *input = bits.unread;
        self.bits = bits.bits;
        self.count = bits.count;
        result
    }

    fn advance(&mut self, input: &mut SliceBits<'_>, output: &mut [u8]) -> io::Result<usize> {
        macro_rules! take {
            ($n:expr) => {
                match input.take($n) {
                    Some(v) => v,
                    None => return Ok(0),
                }
            };
        }
        loop {
            match self.stage {
                Stage::Header(index) => {
                    let byte = take!(8) as u8;
                    if index < 3 {
                        if byte != b"BZh"[index] {
                            return Err(invalid("invalid BZip2 header"));
                        }
                        self.stage = Stage::Header(index + 1);
                    } else {
                        if !(b'1'..=b'9').contains(&byte) {
                            return Err(invalid("invalid BZip2 block size"));
                        }
                        self.limit = usize::from(byte - b'0') * 100_000;
                        self.stage = Stage::MagicHigh;
                    }
                }
                Stage::MagicHigh => {
                    let high = take!(24);
                    self.stage = Stage::MagicLow(high);
                }
                Stage::MagicLow(high) => {
                    let magic = (u64::from(high) << 24) | u64::from(take!(24));
                    self.stage = match magic {
                        STREAM_END_MAGIC => Stage::Footer,
                        BLOCK_MAGIC => {
                            self.header = BlockHeader::default();
                            Stage::BlockHeader
                        }
                        _ => return Err(invalid("invalid BZip2 block marker")),
                    };
                }
                Stage::Footer => {
                    if take!(32) != self.combined {
                        return Err(invalid("BZip2 combined CRC mismatch"));
                    }
                    self.stage = Stage::Done;
                    return Ok(0);
                }
                Stage::BlockHeader => {
                    if !self.header.read(input, &mut self.workspace)? {
                        return Ok(0);
                    }
                    self.body = Some(BlockBody::new(&self.workspace.alphabet));
                    self.column.clear();
                    self.stage = Stage::Symbols;
                }
                Stage::Symbols => {
                    if !self.read_symbols(input)? {
                        return Ok(0);
                    }
                    self.stage = Stage::Output;
                }
                Stage::Output => {
                    let active = self.active.as_mut().expect("active output block");
                    if active.finished() {
                        self.stage = Stage::MagicHigh;
                        continue;
                    }
                    if output.is_empty() {
                        return Ok(0);
                    }
                    let before = active.delivered();
                    let result = active.read(output);
                    self.delivered += active.delivered() - before;
                    return result;
                }
                Stage::Done => return Ok(0),
            }
        }
    }

    fn read_symbols(&mut self, input: &mut SliceBits<'_>) -> io::Result<bool> {
        let result = self
            .body
            .as_mut()
            .expect("active block parser")
            .read::<false>(input, &self.workspace, &mut self.column, self.limit);
        // Suspension keeps the incomplete code, including its whole bytes.
        if matches!(result, Ok(None)) {
            return Ok(false);
        }
        input.release_lookahead();
        result?;

        let block = self.header.finish(
            self.body.take().expect("completed block parser"),
            std::mem::take(&mut self.column),
            self.limit,
        )?;
        self.combined = self.combined.rotate_left(1) ^ block.crc;
        let links = self
            .active
            .take()
            .map_or_else(|| Links::new(self.mode), BlockOutput::into_links);
        self.active = Some(BlockOutput::new(&block, links));
        self.column = block.last;
        Ok(true)
    }
}
