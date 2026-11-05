//! Resumable form of Parser::next for caller-owned input slices.
// Translation of BZip2Decoder.cpp from 7-Zip, LGPL-2.1-or-later.
// Changes: explicit suspension points; shared Huffman validation and block output.
use super::{Block, BlockOutput, Huffman, Links, Mode, invalid};
use std::io;

#[derive(Clone, Copy)]
enum Stage {
    Header(usize),
    MagicHigh,
    MagicLow(u32),
    Footer,
    Crc,
    Random,
    Origin,
    Groups,
    Alphabet(usize),
    TableCount,
    SelectorCount,
    Selectors,
    LengthStart,
    LengthValue,
    LengthDelta,
    Symbols,
    Output,
    Done,
}

pub(crate) struct Incremental {
    stage: Stage,
    bits: u64,
    count: u32,
    limit: usize,
    combined: u32,
    crc: u32,
    randomized: bool,
    origin: usize,
    groups: u32,
    alphabet: Vec<u8>,
    table_count: usize,
    selector_count: usize,
    selector_read: usize,
    selector_unary: usize,
    selector_mtf: [usize; 6],
    selectors: Vec<usize>,
    tables: Vec<Huffman>,
    lengths: Vec<u8>,
    length: i32,
    last: Vec<u8>,
    counts: [usize; 256],
    mtf: [u8; 256],
    selector_index: usize,
    group_left: usize,
    table: usize,
    code: u32,
    code_len: usize,
    run: usize,
    weight: usize,
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
            crc: 0,
            randomized: false,
            origin: 0,
            groups: 0,
            alphabet: Vec::new(),
            table_count: 0,
            selector_count: 0,
            selector_read: 0,
            selector_unary: 0,
            selector_mtf: [0, 1, 2, 3, 4, 5],
            selectors: Vec::new(),
            tables: Vec::new(),
            lengths: Vec::new(),
            length: 0,
            last: Vec::new(),
            counts: [0; 256],
            mtf: [0; 256],
            selector_index: 0,
            group_left: 0,
            table: 0,
            code: 0,
            code_len: 0,
            run: 0,
            weight: 1,
            mode: if small {
                Mode::Compact
            } else {
                Mode::platform()
            },
            active: None,
            delivered: 0,
        }
    }
    fn take(&mut self, input: &mut &[u8], n: u32) -> Option<u32> {
        while self.count < n {
            let (&byte, rest) = input.split_first()?;
            *input = rest;
            self.bits = (self.bits << 8) | u64::from(byte);
            self.count += 8;
        }
        self.count -= n;
        Some(((self.bits >> self.count) & ((1u64 << n) - 1)) as u32)
    }
    pub(crate) fn delivered(&self) -> u64 {
        self.delivered
    }
    pub(crate) fn done(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }
    pub(crate) fn process(&mut self, input: &mut &[u8], output: &mut [u8]) -> io::Result<usize> {
        // Every successful take commits one complete field. Partial reservoirs and
        // selector/Huffman cursors belong to this object, never to the call stack.
        macro_rules! take {
            ($n:expr) => {
                match self.take(input, $n) {
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
                    let low = take!(24);
                    self.stage = match (high, low) {
                        (0x177245, 0x385090) => Stage::Footer,
                        (0x314159, 0x265359) => Stage::Crc,
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
                Stage::Crc => {
                    self.crc = take!(32);
                    self.stage = Stage::Random;
                }
                Stage::Random => {
                    self.randomized = take!(1) != 0;
                    self.stage = Stage::Origin;
                }
                Stage::Origin => {
                    self.origin = take!(24) as usize;
                    self.stage = Stage::Groups;
                }
                Stage::Groups => {
                    self.groups = take!(16);
                    self.alphabet.clear();
                    self.stage = Stage::Alphabet(0);
                }
                Stage::Alphabet(group) => {
                    if group == 16 {
                        if self.alphabet.is_empty() {
                            return Err(invalid("empty BZip2 alphabet"));
                        }
                        self.stage = Stage::TableCount;
                    } else {
                        if self.groups & (1 << (15 - group)) != 0 {
                            let mask = take!(16);
                            for low in 0..16 {
                                if mask & (1 << (15 - low)) != 0 {
                                    self.alphabet.push((group * 16 + low) as u8);
                                }
                            }
                        }
                        self.stage = Stage::Alphabet(group + 1);
                    }
                }
                Stage::TableCount => {
                    self.table_count = take!(3) as usize;
                    if !(2..=6).contains(&self.table_count) {
                        return Err(invalid("invalid BZip2 Huffman groups"));
                    }
                    self.stage = Stage::SelectorCount;
                }
                Stage::SelectorCount => {
                    self.selector_count = take!(15) as usize;
                    if self.selector_count == 0 {
                        return Err(invalid("invalid BZip2 Huffman groups"));
                    }
                    self.selectors.clear();
                    self.selector_read = 0;
                    self.selector_unary = 0;
                    self.selector_mtf = [0, 1, 2, 3, 4, 5];
                    self.tables.clear();
                    self.stage = Stage::Selectors;
                }
                Stage::Selectors => {
                    if self.selector_read == self.selector_count {
                        self.stage = Stage::LengthStart;
                        continue;
                    }
                    if take!(1) != 0 {
                        self.selector_unary += 1;
                        if self.selector_unary >= self.table_count {
                            return Err(invalid("invalid BZip2 selector"));
                        }
                    } else {
                        let selected = self.selector_mtf[self.selector_unary];
                        self.selector_mtf[..=self.selector_unary].rotate_right(1);
                        if self.selectors.len() < 18002 {
                            self.selectors.push(selected);
                        }
                        self.selector_unary = 0;
                        self.selector_read += 1;
                    }
                }
                Stage::LengthStart => {
                    if self.tables.len() == self.table_count {
                        self.mtf[..self.alphabet.len()].copy_from_slice(&self.alphabet);
                        self.last.clear();
                        self.counts.fill(0);
                        self.selector_index = 0;
                        self.group_left = 0;
                        self.code = 0;
                        self.code_len = 0;
                        self.run = 0;
                        self.weight = 1;
                        self.stage = Stage::Symbols;
                    } else {
                        self.length = take!(5) as i32;
                        if !(1..=20).contains(&self.length) {
                            return Err(invalid("invalid Huffman length"));
                        }
                        self.lengths.clear();
                        self.stage = Stage::LengthValue;
                    }
                }
                Stage::LengthValue => {
                    if take!(1) != 0 {
                        self.stage = Stage::LengthDelta;
                    } else {
                        if !(1..=20).contains(&self.length) {
                            return Err(invalid("invalid Huffman length"));
                        }
                        self.lengths.push(self.length as u8);
                        if self.lengths.len() == self.alphabet.len() + 2 {
                            self.tables.push(Huffman::new(&self.lengths)?);
                            self.stage = Stage::LengthStart;
                        }
                    }
                }
                Stage::LengthDelta => {
                    self.length += if take!(1) == 0 { 1 } else { -1 };
                    if !(1..=20).contains(&self.length) {
                        return Err(invalid("invalid Huffman length delta"));
                    }
                    self.stage = Stage::LengthValue;
                }
                Stage::Symbols => {
                    if self.group_left == 0 {
                        self.table = *self
                            .selectors
                            .get(self.selector_index)
                            .ok_or_else(|| invalid("missing BZip2 selector"))?;
                        self.selector_index += 1;
                        self.group_left = 50;
                    }
                    self.code = (self.code << 1) | take!(1);
                    self.code_len += 1;
                    let table = &self.tables[self.table];
                    let index = self.code.wrapping_sub(table.first[self.code_len]);
                    if index >= table.counts[self.code_len] {
                        if self.code_len == 20 {
                            return Err(invalid("invalid Huffman code"));
                        }
                        continue;
                    }
                    let symbol =
                        usize::from(table.symbols[table.offsets[self.code_len] + index as usize]);
                    self.code = 0;
                    self.code_len = 0;
                    self.group_left -= 1;
                    if symbol <= 1 {
                        self.run = self
                            .run
                            .checked_add(self.weight * (symbol + 1))
                            .ok_or_else(|| invalid("BZip2 run overflow"))?;
                        if self.run > self.limit - self.last.len() {
                            return Err(invalid("BZip2 block exceeds declared size"));
                        }
                        self.weight = self
                            .weight
                            .checked_mul(2)
                            .ok_or_else(|| invalid("BZip2 run overflow"))?;
                        continue;
                    }
                    if self.run != 0 {
                        let byte = self.mtf[0];
                        self.counts[usize::from(byte)] += self.run;
                        self.last.resize(self.last.len() + self.run, byte);
                        self.run = 0;
                        self.weight = 1;
                    }
                    if symbol == self.alphabet.len() + 1 {
                        if self.origin >= self.last.len() {
                            return Err(invalid("invalid BZip2 BWT origin"));
                        }
                        self.combined = self.combined.rotate_left(1) ^ self.crc;
                        let links = self
                            .active
                            .take()
                            .map_or_else(|| Links::new(self.mode), BlockOutput::into_links);
                        self.active = Some(BlockOutput::new(
                            Block {
                                last: std::mem::take(&mut self.last),
                                counts: self.counts,
                                origin: self.origin,
                                randomized: self.randomized,
                                crc: self.crc,
                            },
                            links,
                        ));
                        self.stage = Stage::Output;
                    } else {
                        let index = symbol - 1;
                        if index >= self.alphabet.len() {
                            return Err(invalid("invalid BZip2 MTF index"));
                        }
                        if self.last.len() == self.limit {
                            return Err(invalid("BZip2 block exceeds declared size"));
                        }
                        let byte = self.mtf[index];
                        self.last.push(byte);
                        self.counts[usize::from(byte)] += 1;
                        self.mtf[..=index].rotate_right(1);
                    }
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
}
