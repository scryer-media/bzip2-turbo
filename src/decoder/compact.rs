//! Compact DecodeBlock1 / CSpecState::Decode reconstruction from 7-Zip.
//! SPDX-License-Identifier: LGPL-2.1-or-later
use super::super::tt_storage::TtBuffer;
use super::{Block, OutputState, RANDOM_NUMBERS, invalid, tt};
use std::io;

impl OutputState<u32> {
    pub(super) fn new(block: Block, links: TtBuffer<u32>) -> Self {
        let n = block.last.len();
        let links = tt::build_links(&block.last, block.counts, links);
        let position = links[(links[block.origin] >> 8) as usize];
        Self {
            links,
            position,
            remaining: n,
            previous: 0,
            repeated: 0,
            pending: 0,
            random_index: 1,
            random_count: RANDOM_NUMBERS[0] - 2,
            randomized: block.randomized,
            crc: !0,
            expected_crc: block.crc,
            delivered: 0,
        }
    }
    pub(super) fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.randomized {
            self.read_kind::<true>(output)
        } else {
            self.read_kind::<false>(output)
        }
    }
    fn read_kind<const RANDOM: bool>(&mut self, output: &mut [u8]) -> io::Result<usize> {
        // CSpecState::Decode keeps the dependency chain and RLE state in locals.
        let mut position = self.position;
        let mut remaining = self.remaining;
        let mut previous = self.previous;
        let mut repeated = self.repeated;
        let mut pending = self.pending;
        let mut random_count = self.random_count;
        let mut random_index = self.random_index;
        let links: &[u32] = &self.links;
        let mut written = 0;
        // CSpecState::Decode drains a carried run before the normal-byte loop.
        if pending != 0 {
            let count = pending.min(output.len());
            output[..count].fill(previous);
            written = count;
            pending -= count;
        }
        while written < output.len() && remaining != 0 {
            let mut byte = position as u8;
            position = links[(position >> 8) as usize];
            remaining -= 1;
            if RANDOM {
                if random_count == 0 {
                    byte ^= 1;
                    random_count = RANDOM_NUMBERS[random_index];
                    random_index = (random_index + 1) & 511;
                }
                random_count -= 1;
            }
            if repeated == 4 {
                repeated = 0;
                let count = usize::from(byte).min(output.len() - written);
                output[written..written + count].fill(previous);
                written += count;
                pending = usize::from(byte) - count;
            } else {
                output[written] = byte;
                written += 1;
                if byte == previous {
                    repeated += 1;
                } else {
                    previous = byte;
                    repeated = 1;
                }
            }
        }
        self.position = position;
        self.remaining = remaining;
        self.previous = previous;
        self.repeated = repeated;
        self.pending = pending;
        self.random_count = random_count;
        self.random_index = random_index;
        // Check the delivered bytes in one batch so crc-turbo can select its
        // accelerated BZip2 polynomial kernel before the block CRC is checked.
        self.crc = super::super::update_crc(self.crc, &output[..written]);
        self.delivered += written as u64;
        // CSpecState::Finished accepts a final fourth byte without a repeat count.
        if self.finished() && !self.crc != self.expected_crc {
            return Err(invalid("BZip2 block CRC mismatch"));
        }
        Ok(written)
    }
}
