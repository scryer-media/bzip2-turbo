//! Resumable block grammar from 7-Zip's ReadBlock2.
//! SPDX-License-Identifier: LGPL-2.1-or-later

#[cfg(not(target_arch = "x86_64"))]
use super::move_mtf;
#[cfg(target_arch = "x86_64")]
use super::move_mtf_bytes;
use super::{Block, Huffman, ParserWorkspace, append_huffman, invalid};
use std::io;
use std::ops::Deref;

/// Reader-backed input treats EOF as an error. A caller-owned slice returns
/// None when a field or code needs more input.
pub(super) trait BlockInput {
    fn take(&mut self, count: u32) -> io::Result<Option<u32>>;
    fn symbol<const BYTEWISE: bool>(&mut self, table: &Huffman) -> io::Result<Option<usize>>;
}

const MAX_SELECTORS: usize = 18_002;
const GROUP_SIZE: usize = 50;

#[derive(Default)]
enum Stage {
    #[default]
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
    Done,
}

#[derive(Default)]
pub(super) struct BlockHeader {
    stage: Stage,
    crc: u32,
    randomized: bool,
    origin: usize,
    groups: u32,
    table_count: usize,
    selector_count: usize,
    selector_read: usize,
    selector_rank: usize,
    selector_mtf: [u8; 6],
    table_index: usize,
    length: i32,
}

impl BlockHeader {
    /// Advance from the block CRC to the first Huffman symbol. Table and
    /// selector storage stays with the caller across blocks.
    pub(super) fn read(
        &mut self,
        input: &mut impl BlockInput,
        workspace: &mut ParserWorkspace,
    ) -> io::Result<bool> {
        macro_rules! take {
            ($count:expr) => {
                match input.take($count)? {
                    Some(value) => value,
                    None => return Ok(false),
                }
            };
        }
        loop {
            match self.stage {
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
                    workspace.alphabet.clear();
                    workspace.alphabet.reserve_exact(256);
                    self.stage = Stage::Alphabet(0);
                }
                Stage::Alphabet(group) => {
                    if group == 16 {
                        if workspace.alphabet.is_empty() {
                            return Err(invalid("empty BZip2 alphabet"));
                        }
                        self.stage = Stage::TableCount;
                    } else {
                        if self.groups & (1 << (15 - group)) != 0 {
                            let mask = take!(16);
                            for low in 0..16 {
                                if mask & (1 << (15 - low)) != 0 {
                                    workspace.alphabet.push((16 * group + low) as u8);
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
                    workspace.selectors.clear();
                    workspace
                        .selectors
                        .reserve_exact(self.selector_count.min(MAX_SELECTORS));
                    self.selector_read = 0;
                    self.selector_rank = 0;
                    self.selector_mtf = [0, 1, 2, 3, 4, 5];
                    self.stage = Stage::Selectors;
                }
                Stage::Selectors => {
                    while self.selector_read < self.selector_count {
                        while take!(1) != 0 {
                            self.selector_rank += 1;
                            if self.selector_rank >= self.table_count {
                                return Err(invalid("invalid BZip2 selector"));
                            }
                        }
                        let selected = self.selector_mtf[self.selector_rank];
                        self.selector_mtf[..=self.selector_rank].rotate_right(1);
                        if workspace.selectors.len() < MAX_SELECTORS {
                            workspace.selectors.push(selected);
                        }
                        self.selector_rank = 0;
                        self.selector_read += 1;
                    }
                    workspace
                        .tables
                        .reserve_exact(self.table_count.saturating_sub(workspace.tables.len()));
                    self.table_index = 0;
                    self.stage = Stage::LengthStart;
                }
                Stage::LengthStart => {
                    if self.table_index == self.table_count {
                        self.stage = Stage::Done;
                    } else {
                        self.length = take!(5) as i32;
                        if !(1..=20).contains(&self.length) {
                            return Err(invalid("invalid Huffman length"));
                        }
                        workspace.lengths.clear();
                        workspace
                            .lengths
                            .reserve_exact(workspace.alphabet.len() + 2);
                        self.stage = Stage::LengthValue;
                    }
                }
                Stage::LengthValue => {
                    if take!(1) != 0 {
                        self.stage = Stage::LengthDelta;
                    } else {
                        workspace.lengths.push(self.length as u8);
                        if workspace.lengths.len() == workspace.alphabet.len() + 2 {
                            if self.table_index == workspace.tables.len() {
                                append_huffman(&mut workspace.tables, &workspace.lengths)?;
                            } else {
                                workspace.tables[self.table_index].rebuild(&workspace.lengths)?;
                            }
                            self.table_index += 1;
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
                Stage::Done => return Ok(true),
            }
        }
    }

    pub(super) fn finish<C: Deref<Target = [u8]>>(
        &self,
        body: BlockBody,
        last: C,
        limit: usize,
    ) -> io::Result<Block<C>> {
        if self.origin >= last.len() {
            return Err(invalid("invalid BZip2 BWT origin"));
        }
        Ok(Block {
            last,
            limit,
            counts: body.counts,
            origin: self.origin,
            randomized: self.randomized,
            crc: self.crc,
        })
    }
}

// The native path stores into a pre-sized column. Slice-driven decoding
// grows its column as bytes arrive. Both use the same symbol loop.
pub(super) trait BlockColumn {
    fn literal(&mut self, position: usize, byte: u8);
    fn run(&mut self, position: usize, length: usize, byte: u8);
}

impl BlockColumn for [u8] {
    #[inline(always)]
    fn literal(&mut self, position: usize, byte: u8) {
        self[position] = byte;
    }

    #[inline(always)]
    fn run(&mut self, position: usize, length: usize, byte: u8) {
        self[position..position + length].fill(byte);
    }
}

impl BlockColumn for Vec<u8> {
    #[inline(always)]
    fn literal(&mut self, position: usize, byte: u8) {
        debug_assert_eq!(position, self.len());
        self.push(byte);
    }

    #[inline(always)]
    fn run(&mut self, position: usize, length: usize, byte: u8) {
        debug_assert_eq!(position, self.len());
        self.resize(position + length, byte);
    }
}

#[cfg(target_arch = "x86_64")]
type Mtf = [u8; 256];
#[cfg(not(target_arch = "x86_64"))]
type Mtf = [u64; 32];

pub(super) struct BlockBody {
    counts: [usize; 256],
    mtf: Mtf,
    length: usize,
    selector_index: usize,
    group_left: usize,
    table: usize,
    run: usize,
    weight: usize,
}

impl BlockBody {
    pub(super) fn new(alphabet: &[u8]) -> Self {
        let mut mtf = [0; _];
        #[cfg(target_arch = "x86_64")]
        mtf[..alphabet.len()].copy_from_slice(alphabet);
        #[cfg(not(target_arch = "x86_64"))]
        for (index, &byte) in alphabet.iter().enumerate() {
            mtf[index / 8] |= u64::from(byte) << ((index & 7) * 8);
        }
        Self {
            counts: [0; 256],
            mtf,
            length: 0,
            selector_index: 0,
            group_left: 0,
            table: 0,
            run: 0,
            weight: 1,
        }
    }

    /// Decode until EOB or until the current input slice is exhausted.
    /// The reader-backed instantiation never suspends and keeps its cursors
    /// local to the hot loop. The slice instantiation saves them on suspension.
    #[inline(always)]
    pub(super) fn read<const BYTEWISE: bool>(
        &mut self,
        input: &mut impl BlockInput,
        workspace: &ParserWorkspace,
        last: &mut (impl BlockColumn + ?Sized),
        limit: usize,
    ) -> io::Result<Option<usize>> {
        let alphabet_size = workspace.alphabet.len();
        let selectors = &workspace.selectors;
        let tables = &workspace.tables;
        let mut length = self.length;
        let mut selector_index = self.selector_index;
        let mut group_left = self.group_left;
        let mut table = self.table;
        let mut huffman = &tables[table];
        let mut run = self.run;
        let mut weight = self.weight;
        loop {
            if group_left == 0 {
                table = usize::from(
                    *selectors
                        .get(selector_index)
                        .ok_or_else(|| invalid("missing BZip2 selector"))?,
                );
                huffman = &tables[table];
                selector_index += 1;
                group_left = GROUP_SIZE;
            }
            let Some(symbol) = input.symbol::<BYTEWISE>(huffman)? else {
                self.length = length;
                self.selector_index = selector_index;
                self.group_left = group_left;
                self.table = table;
                self.run = run;
                self.weight = weight;
                return Ok(None);
            };
            group_left -= 1;
            if symbol <= 1 {
                run = run
                    .checked_add(weight * (symbol + 1))
                    .ok_or_else(|| invalid("BZip2 run overflow"))?;
                if run > limit - length {
                    return Err(invalid("BZip2 block exceeds declared size"));
                }
                weight = weight
                    .checked_mul(2)
                    .ok_or_else(|| invalid("BZip2 run overflow"))?;
                continue;
            }
            if run != 0 {
                #[cfg(target_arch = "x86_64")]
                let byte = self.mtf[0];
                #[cfg(not(target_arch = "x86_64"))]
                let byte = self.mtf[0] as u8;
                self.counts[usize::from(byte)] += run;
                last.run(length, run, byte);
                length += run;
                run = 0;
                weight = 1;
            }
            if symbol == alphabet_size + 1 {
                return Ok(Some(length));
            }
            let index = symbol - 1;
            if index >= alphabet_size {
                return Err(invalid("invalid BZip2 MTF index"));
            }
            #[cfg(target_arch = "x86_64")]
            let byte = self.mtf[index];
            #[cfg(not(target_arch = "x86_64"))]
            let (next, byte) = {
                let next = self.mtf[index / 8];
                (next, (next >> ((index & 7) * 8)) as u8)
            };
            if length == limit {
                return Err(invalid("BZip2 block exceeds declared size"));
            }
            // ReadBlock2 publishes the byte and histogram before updating MTF.
            last.literal(length, byte);
            length += 1;
            self.counts[usize::from(byte)] += 1;
            #[cfg(target_arch = "x86_64")]
            move_mtf_bytes(&mut self.mtf, index, byte);
            #[cfg(not(target_arch = "x86_64"))]
            move_mtf(&mut self.mtf, index, next, byte);
        }
    }
}
