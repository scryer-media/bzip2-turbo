//! Multithreaded block decoding.
//!
//! The pipeline follows the design lbzip2 describes for its decompressor
//! (scan, retrieve, decode, reorder); no lbzip2 code is used. The caller reads
//! the input in shared chunks and scans every bit offset for the 48-bit block
//! magic. Each candidate becomes a slot whose job a worker decodes
//! speculatively: entropy decode, TT build and output. A candidate can be a
//! false positive, the magic occurring inside block data, so a chain resolves
//! the slots in input order: a slot is real only when it starts exactly where
//! the previous real block, its stream trailer and any following stream
//! header ended. The caller delivers real slots' output in order and drops
//! the rest. Block and stream CRCs are checked as in the serial decoder.
//!
//! Memory is bounded by the thread budget N: N workers each own one TT and one
//! last column, at most 2N slots are open, each slot buffers at most
//! `OUTPUT_CAP` output bytes, and the caller stops reading `window` bytes past
//! the oldest input it still needs (see `retained`); a worker's job holds at
//! most one lookahead of input. A block whose encoding does not
//! fit in a job's lookahead (no ordinary encoder produces one) switches the
//! decoder to the serial parser from that block on.
//! SPDX-License-Identifier: LGPL-2.1-or-later
use super::super::tt_storage::TtBuffer;
use super::{
    BLOCK_MAGIC, Bits, Block, BlockOutput, Links, MAX_BLOCK, Mode, OutputCallback, ParserWorkspace,
    STREAM_END_MAGIC, chunk_crc32, invalid, observe_output, read_block, read_stream_header,
    wants_crc32,
};
use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

/// Input is read and shared in chunks of this size.
pub(super) const INPUT_CHUNK: usize = 1 << 20;
/// Input bytes a job receives past its candidate start before it is submitted.
/// A 900,000-byte block of incompressible bytes encodes to about 0.91 MB; the
/// format allows longer encodings (up to 20-bit codes and unbounded Huffman
/// length deltas), which fall back to the serial parser.
pub(super) const LOOKAHEAD: usize = 1_310_720;
/// Output bytes one slot may hold before its worker waits for delivery. A
/// 900,000-byte block usually expands to about as many output bytes.
pub(super) const OUTPUT_CAP: usize = 2 << 20;
/// Output chunk size without a callback; callbacks keep their 64 KiB chunks.
const UNOBSERVED_CHUNK: usize = 1 << 20;
/// Open slots per worker.
pub(super) const SLOTS_PER_WORKER: usize = 2;
/// The largest worker count; larger budgets are clamped to it.
pub(crate) const MAX_WORKERS: usize = 64;
const MASK48: u64 = (1 << 48) - 1;

/// Input the caller buffers past the oldest byte it must keep (the oldest open
/// slot, the next candidate or the chain's expected block) before it stops
/// reading: a lookahead for every open slot and two more.
pub(super) const fn window(slots: usize) -> u64 {
    (slots as u64 + 2) * LOOKAHEAD as u64
}

/// The most input the caller retains with `slots` open slots: the window, the
/// chunk read when the window was just short, and the partial chunk before
/// the oldest kept byte.
pub(super) const fn retained(slots: usize) -> u64 {
    window(slots) + 2 * INPUT_CHUNK as u64
}

/// One candidate block start and everything decoded from it so far.
struct Slot {
    /// Bit offset of the candidate's block magic.
    start: u64,
    /// The job until a worker takes it.
    job: Option<Job>,
    /// Published by the worker after the entropy decode; taken by the chain.
    parsed: Option<Parsed>,
    verdict: Verdict,
    /// Set by the chain on a real slot that yields no output.
    before: Option<Stop>,
    /// Set by the chain on a real slot after which decoding stops.
    after: Option<Stop>,
    /// Observed output, ready for delivery in order.
    chunks: VecDeque<Vec<u8>>,
    /// With a callback: output produced but not yet observed, oldest first,
    /// each with its CRC-32 when the callback wants one.
    unobserved: VecDeque<(Vec<u8>, Option<u32>)>,
    /// A chunk of this slot is with the callback right now.
    observing: bool,
    /// Bytes in `chunks` and `unobserved`; the worker waits at `OUTPUT_CAP`.
    buffered: usize,
    output: Progress,
}

impl Slot {
    /// Whether every byte this slot will deliver has been produced and
    /// observed: the next slot's output is the next the callback may see.
    fn observed(&self) -> bool {
        matches!(self.output, Progress::Done) && self.unobserved.is_empty() && !self.observing
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verdict {
    Pending,
    Real,
    Discarded,
}

enum Stop {
    End,
    Error(io::Error),
    /// The block needs more input than its job held: decode it serially.
    Serial,
}

enum Progress {
    Pending,
    Done,
    Failed(io::Error),
}

enum Parsed {
    Block { length: usize, crc: u32, gap: Gap },
    Failed(io::Error),
    NeedMore,
}

/// What follows a block, or the start of input. Empty streams are consumed.
struct Gap {
    /// The stored CRC of a stream ended right after the block; only the chain
    /// knows that stream's combined CRC.
    stream_crc: Option<u32>,
    next: io::Result<Next>,
}

enum Next {
    /// The next block magic, and the new block limit if a stream header
    /// preceded it.
    Block {
        start: u64,
        limit: Option<usize>,
    },
    End,
}

/// Input a worker reads one candidate from.
struct Job {
    reader: ChunkReader,
}

/// In-order resolution of the candidate slots.
struct Chain {
    /// Bit offset of the next real block; None before the first stream header.
    expected: Option<u64>,
    limit: usize,
    combined: u32,
    /// Sequence number of the next slot to resolve.
    resolve: u64,
    /// The lowest slot whose output is not yet wholly produced and observed;
    /// with a callback only that slot's chunks may be observed.
    open: u64,
    /// The chain has ended, failed or fallen back; later slots are discarded.
    stopped: bool,
}

struct State {
    slots: VecDeque<Slot>,
    /// Sequence number of `slots[0]`.
    base: u64,
    /// Untaken jobs, oldest first. Workers take them in this order, so the
    /// oldest open slot always has, or is first in line for, a worker.
    queue: VecDeque<u64>,
    chain: Chain,
    cancelled: bool,
    /// Workers waiting for a job.
    idle: usize,
    /// A worker is handing chunks to the callback; the others leave it to it.
    observer: bool,
    spare: Vec<Vec<u8>>,
}

impl State {
    fn slot(&mut self, seq: u64) -> Option<&mut Slot> {
        let index = usize::try_from(seq.checked_sub(self.base)?).ok()?;
        self.slots.get_mut(index)
    }

    /// Resolves parsed slots in order and returns whether output permission
    /// (`open`) moved.
    fn advance(&mut self) -> bool {
        let base = self.base;
        let chain = &mut self.chain;
        while !chain.stopped {
            let Some(expected) = chain.expected else {
                break;
            };
            let limit = chain.limit;
            let Some(slot) = usize::try_from(chain.resolve - base)
                .ok()
                .and_then(|index| self.slots.get_mut(index))
            else {
                break;
            };
            if slot.start < expected {
                // A magic inside the previous real block's data.
                slot.verdict = Verdict::Discarded;
                slot.job = None;
                chain.resolve += 1;
                continue;
            }
            if slot.start > expected {
                // Every candidate before this one was scanned and submitted,
                // so no block magic sits where the chain expects one.
                slot.verdict = Verdict::Real;
                slot.before = Some(Stop::Error(invalid("invalid BZip2 block marker")));
                chain.stopped = true;
                break;
            }
            let Some(parsed) = slot.parsed.take() else {
                break;
            };
            slot.verdict = Verdict::Real;
            chain.resolve += 1;
            match parsed {
                Parsed::Failed(error) => {
                    slot.before = Some(Stop::Error(error));
                    chain.stopped = true;
                }
                Parsed::NeedMore => {
                    slot.before = Some(Stop::Serial);
                    chain.stopped = true;
                }
                Parsed::Block { length, crc, gap } => {
                    // Jobs decode against the format maximum; the stream's
                    // declared limit is enforced here.
                    if length > limit {
                        slot.before =
                            Some(Stop::Error(invalid("BZip2 block exceeds declared size")));
                        chain.stopped = true;
                        break;
                    }
                    let combined = chain.combined.rotate_left(1) ^ crc;
                    chain.combined = combined;
                    if gap.stream_crc.is_some_and(|stored| stored != combined) {
                        slot.after = Some(Stop::Error(invalid("BZip2 combined CRC mismatch")));
                        chain.stopped = true;
                        break;
                    }
                    match gap.next {
                        Ok(Next::Block { start, limit }) => {
                            chain.expected = Some(start);
                            if let Some(limit) = limit {
                                chain.limit = limit;
                                chain.combined = 0;
                            }
                        }
                        Ok(Next::End) => {
                            slot.after = Some(Stop::End);
                            chain.stopped = true;
                        }
                        Err(error) => {
                            slot.after = Some(Stop::Error(error));
                            chain.stopped = true;
                        }
                    }
                }
            }
        }
        if self.chain.stopped {
            let resolve = self.chain.resolve;
            for slot in self
                .slots
                .iter_mut()
                .skip(usize::try_from(resolve.saturating_sub(self.base)).unwrap_or(usize::MAX))
            {
                if slot.verdict == Verdict::Pending {
                    slot.verdict = Verdict::Discarded;
                    slot.job = None;
                }
            }
        }
        self.advance_open()
    }

    fn advance_open(&mut self) -> bool {
        let before = self.chain.open;
        self.chain.open = self.chain.open.max(self.base);
        while let Some(slot) = self.slot(self.chain.open) {
            let complete = match slot.verdict {
                Verdict::Discarded => true,
                Verdict::Real => slot.before.is_none() && slot.observed(),
                Verdict::Pending => false,
            };
            if !complete {
                break;
            }
            self.chain.open += 1;
        }
        self.chain.open != before
    }
}

struct Shared {
    state: Mutex<State>,
    /// Workers wait here for jobs, output space, output permission or cancel.
    work: Condvar,
    /// The caller waits here for slot progress.
    ready: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Callbacks and codec work never run under this lock, so a poisoned
        // state is still consistent.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A job's input: shared chunks from the candidate's byte onward.
pub(super) struct ChunkReader {
    chunks: Vec<Arc<Vec<u8>>>,
    index: usize,
    offset: usize,
    /// Absolute byte offset of the first byte this reader returns.
    start: u64,
    delivered: u64,
    /// Whether the chunks end at the end of the input.
    complete: bool,
}

impl ChunkReader {
    fn empty() -> Self {
        Self {
            chunks: Vec::new(),
            index: 0,
            offset: 0,
            start: 0,
            delivered: 0,
            complete: true,
        }
    }
}

impl Read for ChunkReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        while let Some(chunk) = self.chunks.get(self.index) {
            let available = &chunk[self.offset..];
            if available.is_empty() {
                self.index += 1;
                self.offset = 0;
                continue;
            }
            let count = available.len().min(output.len());
            output[..count].copy_from_slice(&available[..count]);
            self.offset += count;
            self.delivered += count as u64;
            return Ok(count);
        }
        Ok(0)
    }
}

/// Absolute bit offset of the next unconsumed bit.
fn position(bits: &Bits<ChunkReader>) -> u64 {
    (bits.reader.start + bits.reader.delivered) * 8
        - (bits.end - bits.pos) as u64 * 8
        - u64::from(bits.count)
}

/// Whether an error means the job's chunks ended before the input did.
fn needs_more(error: &io::Error, reader: &ChunkReader) -> bool {
    error.kind() == io::ErrorKind::UnexpectedEof && !reader.complete
}

/// Reads stream trailers and headers up to the next block magic.
/// `at_start` reads the first stream header; otherwise a block just ended.
fn read_gap(bits: &mut Bits<ChunkReader>, at_start: bool, multistream: bool) -> Option<Gap> {
    fn inner(
        bits: &mut Bits<ChunkReader>,
        at_start: bool,
        multistream: bool,
        stream_crc: &mut Option<u32>,
    ) -> io::Result<Option<Next>> {
        let mut limit = None;
        // The combined CRC of the current stream when this gap began it.
        let mut combined = None;
        if at_start {
            limit = read_stream_header(bits, true)?;
            combined = Some(0);
        }
        loop {
            let start = position(bits);
            let magic = (u64::from(bits.take(24)?) << 24) | u64::from(bits.take(24)?);
            if magic == BLOCK_MAGIC {
                return Ok(Some(Next::Block { start, limit }));
            }
            if magic != STREAM_END_MAGIC {
                return Err(invalid("invalid BZip2 block marker"));
            }
            let stored = bits.take(32)?;
            match combined {
                Some(combined) if stored != combined => {
                    return Err(invalid("BZip2 combined CRC mismatch"));
                }
                Some(_) => {}
                None => *stream_crc = Some(stored),
            }
            if !multistream {
                bits.align();
                return Ok(Some(Next::End));
            }
            match read_stream_header(bits, false)? {
                Some(next) => {
                    limit = Some(next);
                    combined = Some(0);
                }
                // The chunks ran out, not necessarily the input.
                None if !bits.reader.complete => return Ok(None),
                None => return Ok(Some(Next::End)),
            }
        }
    }
    let mut stream_crc = None;
    let next = match inner(bits, at_start, multistream, &mut stream_crc) {
        Ok(Some(next)) => Ok(next),
        Ok(None) => return None,
        Err(error) if needs_more(&error, &bits.reader) => return None,
        Err(error) => Err(error),
    };
    Some(Gap { stream_crc, next })
}

/// Decodes the candidate block at `start` and the gap after it.
fn parse_job(
    bits: &mut Bits<ChunkReader>,
    workspace: &mut ParserWorkspace,
    column: &mut TtBuffer<u8>,
    start: u64,
    multistream: bool,
) -> (Parsed, Option<Block>) {
    let result = (|| {
        bits.take((start % 8) as u32)?;
        let magic = (u64::from(bits.take(24)?) << 24) | u64::from(bits.take(24)?);
        if magic != BLOCK_MAGIC {
            return Err(invalid("invalid BZip2 block marker"));
        }
        // A failed parse drops the column and the worker's next job allocates
        // a fresh one. Only false-positive magics and blocks beyond the
        // lookahead fail here, so ordinary streams reuse it every block.
        read_block(bits, workspace, std::mem::take(column), MAX_BLOCK)
    })();
    let block = match result {
        Ok(block) => block,
        Err(error) if needs_more(&error, &bits.reader) => return (Parsed::NeedMore, None),
        Err(error) => return (Parsed::Failed(error), None),
    };
    let Some(gap) = read_gap(bits, false, multistream) else {
        *column = block.last;
        return (Parsed::NeedMore, None);
    };
    (
        Parsed::Block {
            length: block.last.len(),
            crc: block.crc,
            gap,
        },
        Some(block),
    )
}

/// Byte masks of the eight bit alignments at which the magic's second and
/// third bytes take each value: bit `s` of `FILTER[k][b]` is set when byte
/// `k + 1` of a window whose magic starts at bit `s` equals `b`.
const FILTER: [[u8; 256]; 2] = {
    let mut tables = [[0u8; 256]; 2];
    let mut shift = 0;
    while shift < 8 {
        let pattern = BLOCK_MAGIC << (16 - shift);
        tables[0][((pattern >> 48) & 0xff) as usize] |= 1 << shift;
        tables[1][((pattern >> 40) & 0xff) as usize] |= 1 << shift;
        shift += 1;
    }
    tables
};

/// Appends the bit offsets of every block magic that starts in
/// `bytes[from..to]`; each start needs eight readable bytes.
fn scan(bytes: &[u8], base: u64, from: usize, to: usize, found: &mut VecDeque<u64>) {
    debug_assert!(to + 7 <= bytes.len() || from >= to);
    for index in from..to {
        let mask =
            FILTER[0][usize::from(bytes[index + 1])] & FILTER[1][usize::from(bytes[index + 2])];
        if mask != 0 {
            let word = u64::from_be_bytes(bytes[index..index + 8].try_into().unwrap());
            for shift in 0..8 {
                if mask & (1 << shift) != 0 && (word >> (16 - shift)) & MASK48 == BLOCK_MAGIC {
                    found.push_back((base + index as u64) * 8 + shift);
                }
            }
        }
    }
}

/// Caller-side input: shared chunks, the scan position and pending candidates.
struct Input {
    chunks: VecDeque<Arc<Vec<u8>>>,
    /// Absolute offset of the first retained byte.
    base: u64,
    /// Absolute offset one past the last buffered byte.
    end: u64,
    eof: bool,
    /// Every candidate starting before this byte has been found.
    scanned: u64,
    candidates: VecDeque<u64>,
}

impl Input {
    fn new() -> Self {
        Self {
            chunks: VecDeque::new(),
            base: 0,
            end: 0,
            eof: false,
            scanned: 0,
            candidates: VecDeque::new(),
        }
    }

    fn fill<R: Read>(&mut self, reader: &mut R) -> io::Result<()> {
        let mut chunk = vec![0; INPUT_CHUNK];
        let mut filled = 0;
        while filled < chunk.len() {
            match reader.read(&mut chunk[filled..]) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(count) => filled += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        if filled > 0 {
            chunk.truncate(filled);
            self.end += filled as u64;
            self.chunks.push_back(Arc::new(chunk));
        }
        self.scan();
        Ok(())
    }

    /// Scans every start with eight bytes buffered, or every start at EOF.
    fn scan(&mut self) {
        let limit = if self.eof {
            self.end
        } else {
            self.end.saturating_sub(7)
        };
        let mut chunk_base = self.base;
        for (index, chunk) in self.chunks.iter().enumerate() {
            let chunk_end = chunk_base + chunk.len() as u64;
            if self.scanned >= limit {
                break;
            }
            if self.scanned < chunk_end {
                let from = (self.scanned - chunk_base) as usize;
                let to = chunk
                    .len()
                    .saturating_sub(7)
                    .min((limit - chunk_base) as usize);
                if from < to {
                    scan(chunk, chunk_base, from, to, &mut self.candidates);
                    self.scanned = chunk_base + to as u64;
                }
                // Starts whose window crosses into later chunks, or past EOF.
                while self.scanned < chunk_end.min(limit) {
                    let mut window = [0u8; 8];
                    let mut filled = 0;
                    let mut position = self.scanned;
                    // Copy up to eight bytes starting at `position`.
                    let mut cursor = chunk_base;
                    for later in self.chunks.range(index..) {
                        let later_end = cursor + later.len() as u64;
                        while filled < 8 && position < later_end {
                            window[filled] = later[(position - cursor) as usize];
                            filled += 1;
                            position += 1;
                        }
                        cursor = later_end;
                        if filled == 8 {
                            break;
                        }
                    }
                    let start = self.scanned;
                    let word = u64::from_be_bytes(window);
                    for shift in 0..8u64 {
                        // A magic must lie wholly inside the buffered input.
                        if start * 8 + shift + 48 <= self.end * 8
                            && (word >> (16 - shift)) & MASK48 == BLOCK_MAGIC
                        {
                            self.candidates.push_back(start * 8 + shift);
                        }
                    }
                    self.scanned += 1;
                }
            }
            chunk_base = chunk_end;
        }
    }

    /// A reader over the buffered bytes from `byte` onward, in whole chunks up
    /// to the first that reaches `byte + span`.
    fn reader(&self, byte: u64, span: u64) -> ChunkReader {
        debug_assert!(byte >= self.base);
        let mut chunks = Vec::new();
        let mut offset = 0;
        let mut chunk_base = self.base;
        let mut complete = self.eof;
        for chunk in &self.chunks {
            if chunk_base >= byte.saturating_add(span) {
                complete = false;
                break;
            }
            let chunk_end = chunk_base + chunk.len() as u64;
            if chunk_end > byte {
                if chunks.is_empty() {
                    offset = (byte - chunk_base) as usize;
                }
                chunks.push(Arc::clone(chunk));
            }
            chunk_base = chunk_end;
        }
        ChunkReader {
            chunks,
            index: 0,
            offset,
            start: byte,
            delivered: 0,
            complete,
        }
    }

    /// The buffered bytes from `byte` onward.
    fn bytes_from(&self, byte: u64) -> Vec<u8> {
        let mut reader = self.reader(byte, u64::MAX);
        let mut bytes = Vec::with_capacity((self.end - byte) as usize);
        reader
            .read_to_end(&mut bytes)
            .expect("chunk reads are infallible");
        bytes
    }

    /// Releases chunks wholly before `byte`.
    fn trim(&mut self, byte: u64) {
        while let Some(chunk) = self.chunks.front() {
            let chunk_end = self.base + chunk.len() as u64;
            if chunk_end > byte || chunk_end > self.scanned {
                break;
            }
            self.base = chunk_end;
            self.chunks.pop_front();
        }
    }
}

/// Where serial decoding resumes after a fallback.
pub(super) struct Resume {
    pub(super) bytes: Vec<u8>,
    pub(super) skip: u32,
    pub(super) limit: usize,
    pub(super) combined: u32,
}

pub(super) enum Step {
    Chunk(Vec<u8>),
    End,
    Serial(Resume),
}

/// Worker configuration shared by every worker of one decoder.
#[derive(Clone)]
struct Config {
    mode: Mode,
    callback: Option<OutputCallback>,
    /// Producers take each chunk's CRC-32 for the callback.
    crc32: bool,
    multistream: bool,
    chunk: usize,
}

/// The caller's handle on the workers; dropping it cancels and joins them.
pub(super) struct Pool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    max_workers: usize,
    max_slots: usize,
    config: Config,
    input: Input,
    started: bool,
    next_seq: u64,
}

impl Pool {
    pub(super) fn new(
        threads: usize,
        mode: Mode,
        callback: Option<OutputCallback>,
        multistream: bool,
    ) -> Self {
        let max_workers = threads.clamp(1, MAX_WORKERS);
        let chunk = if callback.is_some() {
            super::OUTPUT_CHUNK
        } else {
            UNOBSERVED_CHUNK
        };
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    slots: VecDeque::new(),
                    base: 0,
                    queue: VecDeque::new(),
                    chain: Chain {
                        expected: None,
                        limit: 0,
                        combined: 0,
                        resolve: 0,
                        open: 0,
                        stopped: false,
                    },
                    cancelled: false,
                    idle: 0,
                    observer: false,
                    spare: Vec::new(),
                }),
                work: Condvar::new(),
                ready: Condvar::new(),
            }),
            workers: Vec::new(),
            max_workers,
            max_slots: max_workers * SLOTS_PER_WORKER,
            config: Config {
                mode,
                crc32: wants_crc32(&callback),
                callback,
                multistream,
                chunk,
            },
            input: Input::new(),
            started: false,
            next_seq: 0,
        }
    }

    /// Parses the first stream header and any empty streams before the first
    /// block. Returns false when the input holds no block.
    fn start<R: Read>(&mut self, reader: &mut R) -> io::Result<bool> {
        let mut bits = Bits::new(ChunkReader::empty());
        loop {
            if !self.input.eof && self.input.end < LOOKAHEAD as u64 {
                self.input.fill(reader)?;
                continue;
            }
            bits.reset(self.input.reader(0, u64::MAX));
            match read_gap(&mut bits, true, self.config.multistream) {
                None if self.input.eof => return Err(invalid("truncated BZip2 stream")),
                None => self.input.fill(reader)?,
                Some(gap) => {
                    let next = gap.next?;
                    let mut state = self.shared.lock();
                    return Ok(match next {
                        Next::Block { start, limit } => {
                            state.chain.expected = Some(start);
                            state.chain.limit = limit.expect("a first stream header");
                            true
                        }
                        Next::End => {
                            state.chain.stopped = true;
                            false
                        }
                    });
                }
            }
        }
    }

    /// Reads input and submits ready candidates until neither can progress
    /// without waiting for the workers.
    fn pump<R: Read>(&mut self, reader: &mut R) -> io::Result<()> {
        loop {
            let (slots, stopped, expected, oldest) = {
                let state = self.shared.lock();
                (
                    state.slots.len(),
                    state.chain.stopped,
                    state.chain.expected,
                    state.slots.front().map(|slot| slot.start),
                )
            };
            if stopped {
                return Ok(());
            }
            let expected = expected.expect("started chain");
            // Candidates the chain has passed are inside real blocks' data.
            while self.input.candidates.front().is_some_and(|&c| c < expected) {
                self.input.candidates.pop_front();
            }
            let keep = oldest
                .into_iter()
                .chain(self.input.candidates.front().copied())
                .chain([expected])
                .min()
                .unwrap_or(expected);
            self.input.trim(keep / 8);
            if slots >= self.max_slots {
                return Ok(());
            }
            if let Some(&candidate) = self.input.candidates.front() {
                if self.input.eof || self.input.end >= candidate / 8 + LOOKAHEAD as u64 {
                    self.input.candidates.pop_front();
                    self.submit(candidate);
                    continue;
                }
            } else if self.input.eof {
                return Ok(());
            }
            if self.input.eof {
                return Ok(());
            }
            // Stop at the window. Submitted jobs already hold their lookahead,
            // so the oldest slot resolves and `keep` advances; a candidate at
            // the expected block with no open slot is always within reach.
            if self.input.end - keep / 8 > window(self.max_slots) {
                return Ok(());
            }
            self.input.fill(reader)?;
        }
    }

    fn submit(&mut self, start: u64) {
        let job = Job {
            reader: self.input.reader(start / 8, LOOKAHEAD as u64),
        };
        let seq = self.next_seq;
        self.next_seq += 1;
        let spawn = {
            let mut state = self.shared.lock();
            debug_assert_eq!(state.base + state.slots.len() as u64, seq);
            state.slots.push_back(Slot {
                start,
                job: Some(job),
                parsed: None,
                verdict: Verdict::Pending,
                before: None,
                after: None,
                chunks: VecDeque::new(),
                unobserved: VecDeque::new(),
                observing: false,
                buffered: 0,
                output: Progress::Pending,
            });
            state.queue.push_back(seq);
            // Spawn only while queued jobs outnumber idle workers.
            let spawn = state.queue.len() > state.idle && self.workers.len() < self.max_workers;
            self.shared.work.notify_one();
            spawn
        };
        if spawn {
            let shared = Arc::clone(&self.shared);
            let config = self.config.clone();
            match std::thread::Builder::new()
                .name("bzip2-decode".into())
                .spawn(move || worker(&shared, &config))
            {
                Ok(handle) => self.workers.push(handle),
                // Existing workers still take the job; with none, the
                // caller reports the failure when it waits.
                Err(_) if !self.workers.is_empty() => {}
                Err(error) => {
                    let mut state = self.shared.lock();
                    if let Some(slot) = state.slot(seq) {
                        slot.job = None;
                        slot.parsed = Some(Parsed::Failed(error));
                    }
                    state.queue.retain(|&queued| queued != seq);
                    state.advance();
                }
            }
        }
    }

    /// Returns the next output chunk in stream order.
    pub(super) fn next<R: Read>(&mut self, reader: &mut R, recycled: Vec<u8>) -> io::Result<Step> {
        if recycled.capacity() >= self.config.chunk {
            self.shared.lock().spare.push(recycled);
        }
        if !self.started {
            self.started = true;
            if !self.start(reader)? {
                return Ok(Step::End);
            }
        }
        loop {
            self.pump(reader)?;
            let mut state = self.shared.lock();
            let mut popped = false;
            while let Some(front) = state.slots.front_mut() {
                match front.verdict {
                    Verdict::Pending => break,
                    Verdict::Discarded => {
                        state.slots.pop_front();
                        state.base += 1;
                        popped = true;
                        continue;
                    }
                    Verdict::Real => {}
                }
                match front.before.take() {
                    Some(Stop::Error(error)) => return Err(error),
                    Some(Stop::Serial) => {
                        let start = front.start;
                        let (limit, combined) = (state.chain.limit, state.chain.combined);
                        drop(state);
                        return Ok(Step::Serial(Resume {
                            bytes: self.input.bytes_from(start / 8),
                            skip: (start % 8) as u32,
                            limit,
                            combined,
                        }));
                    }
                    Some(Stop::End) => unreachable!("an end follows a block's output"),
                    None => {}
                }
                if let Some(chunk) = front.chunks.pop_front() {
                    front.buffered -= chunk.len();
                    self.shared.work.notify_all();
                    return Ok(Step::Chunk(chunk));
                }
                if !front.unobserved.is_empty() || front.observing {
                    // A worker is observing them; the chunks come next.
                    break;
                }
                match std::mem::replace(&mut front.output, Progress::Pending) {
                    Progress::Pending => break,
                    Progress::Failed(error) => return Err(error),
                    Progress::Done => {
                        let after = front.after.take();
                        state.slots.pop_front();
                        state.base += 1;
                        popped = true;
                        match after {
                            None => {}
                            Some(Stop::End) => return Ok(Step::End),
                            Some(Stop::Error(error)) => return Err(error),
                            Some(Stop::Serial) => unreachable!("serial resumes before output"),
                        }
                    }
                }
            }
            if popped {
                // Freed slots may admit more candidates.
                continue;
            }
            if state.slots.is_empty() {
                // `pump` could neither submit nor read: the input ends, or no
                // magic follows, where the chain expects a block.
                return Err(invalid("invalid BZip2 block marker"));
            }
            if self.workers.is_empty() {
                return Err(io::Error::other(
                    "BZip2 worker stopped before finishing a block",
                ));
            }
            drop(
                self.shared
                    .ready
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            );
        }
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> Snapshot {
        let state = self.shared.lock();
        Snapshot {
            workers: self.workers.len(),
            slots: state.slots.len(),
            buffered: state
                .slots
                .iter()
                .map(|slot| slot.buffered)
                .max()
                .unwrap_or(0),
            retained: self.input.end - self.input.base,
            max_slots: self.max_slots,
            max_workers: self.max_workers,
        }
    }
}

#[cfg(test)]
pub(super) struct Snapshot {
    pub(super) workers: usize,
    pub(super) slots: usize,
    pub(super) buffered: usize,
    pub(super) retained: u64,
    pub(super) max_slots: usize,
    pub(super) max_workers: usize,
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.lock().cancelled = true;
        self.shared.work.notify_all();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

/// Worker-owned buffers, reused across jobs.
struct Scratch {
    bits: Bits<ChunkReader>,
    workspace: ParserWorkspace,
    column: TtBuffer<u8>,
    links: Links,
}

fn worker(shared: &Shared, config: &Config) {
    let mut scratch = Scratch {
        bits: Bits::new(ChunkReader::empty()),
        workspace: ParserWorkspace::default(),
        column: TtBuffer::default(),
        links: Links::new(config.mode),
    };
    loop {
        let (seq, job) = {
            let mut state = shared.lock();
            loop {
                if state.cancelled {
                    return;
                }
                if let Some(seq) = state.queue.pop_front() {
                    if let Some(job) = state.slot(seq).and_then(|slot| slot.job.take()) {
                        break (seq, job);
                    }
                    continue;
                }
                state.idle += 1;
                state = shared
                    .work
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
                state.idle = state.idle.saturating_sub(1);
            }
        };
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_job(shared, config, &mut scratch, seq, job);
        }));
        if run.is_err() {
            // A callback (or codec) panic fails this slot; the decoder reports
            // it in order. The scratch buffers may be mid-update.
            scratch = Scratch {
                bits: Bits::new(ChunkReader::empty()),
                workspace: ParserWorkspace::default(),
                column: TtBuffer::default(),
                links: Links::new(config.mode),
            };
            let mut state = shared.lock();
            if let Some(slot) = state.slot(seq) {
                let error = io::Error::other("BZip2 worker panicked");
                if slot.verdict == Verdict::Pending && slot.parsed.is_none() {
                    slot.parsed = Some(Parsed::Failed(error));
                } else {
                    slot.output = Progress::Failed(error);
                }
            }
            if state.advance() {
                shared.work.notify_all();
            }
            shared.ready.notify_all();
            // The chain may have moved onto chunks waiting to be observed.
            drop(observe(shared, config, state));
        }
    }
}

fn run_job(shared: &Shared, config: &Config, scratch: &mut Scratch, seq: u64, job: Job) {
    let start = {
        let mut state = shared.lock();
        match state.slot(seq) {
            Some(slot) if slot.verdict != Verdict::Discarded => slot.start,
            _ => return,
        }
    };
    scratch.bits.reset(job.reader);
    let (parsed, block) = parse_job(
        &mut scratch.bits,
        &mut scratch.workspace,
        &mut scratch.column,
        start,
        config.multistream,
    );
    // Release the input chunks before the long reconstruction.
    scratch.bits.reset(ChunkReader::empty());
    {
        let mut state = shared.lock();
        let Some(slot) = state.slot(seq) else {
            return;
        };
        slot.parsed = Some(parsed);
        if state.advance() {
            shared.work.notify_all();
        }
        shared.ready.notify_all();
        // Resolving the chain can make chunks other workers produced
        // observable.
        drop(observe(shared, config, state));
    }
    let Some(block) = block else {
        return;
    };
    let mut output = BlockOutput::new(&block, scratch.links.take());
    // The TT is built; the column is free for the next job.
    scratch.column = block.last;
    produce(shared, config, seq, &mut output);
    scratch.links = output.into_links();
}

/// Writes one slot's output chunks. Every worker produces as soon as its slot
/// has room, callback or not; with a callback the chunks wait in the slot,
/// with their CRC-32 when it is wanted, until `observe` hands them over in
/// stream order.
fn produce(shared: &Shared, config: &Config, seq: u64, output: &mut BlockOutput) {
    loop {
        let mut chunk = {
            let mut state = shared.lock();
            loop {
                if state.cancelled {
                    return;
                }
                let Some(slot) = state.slot(seq) else {
                    return;
                };
                if slot.verdict == Verdict::Discarded || !matches!(slot.output, Progress::Pending) {
                    // Discarded, or stopped by a callback error.
                    return;
                }
                if slot.buffered < OUTPUT_CAP {
                    break;
                }
                state = shared
                    .work
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            state.spare.pop().unwrap_or_default()
        };
        chunk.resize(config.chunk, 0);
        let result = output.read(&mut chunk).map(|count| {
            chunk.truncate(count);
            // Taken here, on the worker that produced the bytes, so the
            // ordered calls compute no checksum.
            (count, config.crc32.then(|| chunk_crc32(&chunk)))
        });
        let mut state = shared.lock();
        let finished = output.finished();
        let Some(slot) = state.slot(seq) else {
            return;
        };
        if !matches!(slot.output, Progress::Pending) {
            // A callback error failed this slot while the chunk was made.
            return;
        }
        match result {
            Ok((count, crc)) => {
                slot.buffered += count;
                if config.callback.is_some() {
                    slot.unobserved.push_back((chunk, crc));
                } else {
                    slot.chunks.push_back(chunk);
                }
                if finished {
                    slot.output = Progress::Done;
                } else if count == 0 {
                    slot.output = Progress::Failed(io::Error::other("BZip2 block output stalled"));
                }
            }
            Err(error) => slot.output = Progress::Failed(error),
        }
        let done = !matches!(slot.output, Progress::Pending);
        if config.callback.is_some() {
            state = observe(shared, config, state);
        } else if done && state.advance_open() {
            shared.work.notify_all();
        }
        shared.ready.notify_all();
        drop(state);
        if done {
            return;
        }
    }
}

/// Hands produced chunks to the callback in stream order, from the open slot
/// onward, until the open slot has none waiting. One worker does this at a
/// time; any other finds the token taken and leaves its chunks to that
/// worker, which looks again under the lock before it lets the token go.
fn observe<'a>(
    shared: &'a Shared,
    config: &Config,
    mut state: MutexGuard<'a, State>,
) -> MutexGuard<'a, State> {
    if config.callback.is_none() || state.observer {
        return state;
    }
    state.observer = true;
    loop {
        if state.cancelled {
            break;
        }
        if state.advance_open() {
            shared.work.notify_all();
        }
        let open = state.chain.open;
        let Some(slot) = state.slot(open) else {
            break;
        };
        // Only a slot the chain has resolved as real is observed: a false
        // positive's output is never shown, and nothing after a failed block.
        if slot.verdict != Verdict::Real || slot.before.is_some() {
            break;
        }
        let Some((chunk, crc)) = slot.unobserved.pop_front() else {
            // Not yet produced; its producer looks again when it adds one.
            break;
        };
        slot.observing = true;
        drop(state);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            observe_output(&config.callback, &chunk, crc)
        }))
        .unwrap_or_else(|_| Err(io::Error::other("BZip2 worker panicked")));
        state = shared.lock();
        let Some(slot) = state.slot(open) else {
            // Delivery never removes a slot that is being observed.
            break;
        };
        slot.observing = false;
        match result {
            Ok(()) => slot.chunks.push_back(chunk),
            Err(error) => {
                // Nothing after this chunk is observed or delivered; the
                // producer stops at its next chunk.
                slot.buffered -= chunk.len();
                slot.buffered -= slot.unobserved.iter().map(|(c, _)| c.len()).sum::<usize>();
                slot.unobserved.clear();
                slot.output = Progress::Failed(error);
                shared.work.notify_all();
                shared.ready.notify_all();
                break;
            }
        }
        shared.ready.notify_all();
    }
    state.observer = false;
    state
}

#[cfg(test)]
mod tests;
