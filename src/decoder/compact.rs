//! Compact DecodeBlock1 / CSpecState::Decode reconstruction from 7-Zip.
//! SPDX-License-Identifier: LGPL-2.1-or-later
use super::super::tt_storage::TtBuffer;
use super::{Block, OutputState, RANDOM_NUMBERS, invalid, tt};
use std::io;

/// CSpecState::Decode's `reps` value once four equal bytes have been written:
/// the next symbol is a repeat count rather than a byte.
const REPEAT_COUNT: i32 = -4;

impl OutputState<u32> {
    pub(super) fn new(block: &Block, links: TtBuffer<u32>) -> Self {
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
        let mut written = 0;
        // CSpecState::Decode drains a carried run before the normal-byte loop.
        if self.pending != 0 {
            let count = self.pending.min(output.len());
            output[..count].fill(self.previous);
            written = count;
            self.pending -= count;
        }
        if written < output.len() && self.remaining != 0 {
            written = self.reconstruct::<RANDOM>(output, written);
        }
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
    /// The CSpecState::Decode main loop, entered with `written < output.len()`
    /// and at least one symbol left. Like the reference it keeps only the
    /// output cursor and limit, the chained position, the previous byte and the
    /// signed `reps` countdown live per byte, and saves state only at exit.
    /// While no repeat count arrives every symbol yields one byte, so the block
    /// countdown is carried as the output index where the block's symbols run
    /// out: the reference's fused `data == lim || blockSize == 0` exit becomes
    /// one compare of the output index against the nearer of the two limits.
    #[inline(always)]
    fn reconstruct<const RANDOM: bool>(&mut self, output: &mut [u8], mut written: usize) -> usize {
        let links: &[u32] = &self.links;
        let mut position = self.position;
        // `written + remaining`; only a repeat count moves it.
        let mut block_end = written + self.remaining;
        // Bytes stay zero-extended words, as in the reference's `unsigned b`
        // and `prevByte`, so the comparison never re-masks `previous`.
        let mut previous = u32::from(self.previous);
        // `repeated` counts equal bytes up to four; `reps` is its negation.
        let mut reps = -i32::from(self.repeated);
        let mut random_count = self.random_count;
        let mut random_index = self.random_index;
        loop {
            let limit = output.len().min(block_end);
            let slots = &mut output[..limit];
            let count = loop {
                if written >= slots.len() {
                    break None;
                }
                let mut byte = position & 0xff;
                let link = (position >> 8) as usize;
                if RANDOM {
                    if random_count == 0 {
                        byte ^= 1;
                        random_count = RANDOM_NUMBERS[random_index];
                        random_index = (random_index + 1) & 511;
                    }
                    random_count -= 1;
                }
                // The link is loaded after the byte's last use on each path, so
                // the loaded word can replace `position` in its own register.
                if reps == REPEAT_COUNT {
                    position = links[link];
                    break Some(byte as u8);
                }
                reps = if byte == previous { reps - 1 } else { -1 };
                previous = byte;
                slots[written] = byte as u8;
                written += 1;
                position = links[link];
            };
            if let Some(count) = count {
                // The repeat count is a symbol that writes no byte itself.
                reps = 0;
                let fits = repeat_run(output, written, previous as u8, count, &mut self.pending);
                written += fits;
                block_end = block_end - 1 + fits;
            }
            if written == output.len() || written == block_end {
                break;
            }
        }
        self.position = position;
        self.remaining = block_end - written;
        self.previous = previous as u8;
        self.repeated = (-reps) as u8;
        if RANDOM {
            self.random_count = random_count;
            self.random_index = random_index;
        }
        written
    }
}

/// The repeat-count side block of CSpecState::Decode: writes what fits and
/// carries the rest of the run into the next `read`, returning the
/// number of bytes written.
#[cold]
#[inline(never)]
fn repeat_run(
    output: &mut [u8],
    written: usize,
    previous: u8,
    count: u8,
    pending: &mut usize,
) -> usize {
    let count = usize::from(count);
    let fits = count.min(output.len() - written);
    output[written..written + fits].fill(previous);
    *pending = count - fits;
    fits
}

#[cfg(test)]
mod tests {
    use super::{OutputState, RANDOM_NUMBERS};
    use std::io;

    impl OutputState<u32> {
        /// The previous reconstruction loop, kept as a differential reference.
        fn read_reference(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.randomized {
                self.read_reference_kind::<true>(output)
            } else {
                self.read_reference_kind::<false>(output)
            }
        }
        fn read_reference_kind<const RANDOM: bool>(
            &mut self,
            output: &mut [u8],
        ) -> io::Result<usize> {
            let mut position = self.position;
            let mut remaining = self.remaining;
            let mut previous = self.previous;
            let mut repeated = self.repeated;
            let mut pending = self.pending;
            let mut random_count = self.random_count;
            let mut random_index = self.random_index;
            let links: &[u32] = &self.links;
            let mut written = 0;
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
            self.crc = super::super::super::update_crc(self.crc, &output[..written]);
            self.delivered += written as u64;
            if self.finished() && !self.crc != self.expected_crc {
                return Err(super::invalid("BZip2 block CRC mismatch"));
            }
            Ok(written)
        }
    }

    fn chain(raw: &[u8], randomized: bool, random_count: i32, expected_crc: u32) -> OutputState {
        let links: Vec<u32> = raw
            .iter()
            .enumerate()
            .map(|(i, &byte)| ((((i + 1) % raw.len()) as u32) << 8) | u32::from(byte))
            .collect();
        OutputState {
            position: links[0],
            links: links.into(),
            remaining: raw.len(),
            previous: 0,
            repeated: 0,
            pending: 0,
            random_index: 1,
            random_count,
            randomized,
            crc: !0,
            expected_crc,
            delivered: 0,
        }
    }

    fn state(output: &OutputState) -> (u32, usize, u8, u8, usize, usize, i32, u32, u64) {
        (
            output.position,
            output.remaining,
            output.previous,
            output.repeated,
            output.pending,
            output.random_index,
            output.random_count,
            output.crc,
            output.delivered,
        )
    }

    /// Raw post-BWT symbols with frequent four-byte runs and repeat counts.
    fn symbols(length: usize, seed: u32) -> Vec<u8> {
        let mut next = seed;
        let mut random = move || {
            next = next.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            next >> 8
        };
        let mut raw = Vec::with_capacity(length + 5);
        while raw.len() < length {
            let byte = (random() % 6) as u8;
            if random() % 4 == 0 {
                raw.extend([byte; 4]);
                raw.push(match random() % 4 {
                    0 => 0,
                    1 => 255,
                    _ => (random() % 256) as u8,
                });
            } else {
                raw.push(byte);
            }
        }
        raw.truncate(length);
        raw
    }

    fn compare(raw: &[u8], randomized: bool, random_count: i32, sizes: &[usize]) {
        // Decode once to learn the block CRC, so the final read succeeds.
        let mut probe = chain(raw, randomized, random_count, 0);
        let mut buffer = vec![0; 1 << 16];
        while !probe.finished() {
            if probe.read_reference(&mut buffer).is_err() {
                break;
            }
        }
        let crc = !probe.crc;
        for expected_crc in [crc, crc ^ 1] {
            let mut baseline = chain(raw, randomized, random_count, expected_crc);
            let mut candidate = chain(raw, randomized, random_count, expected_crc);
            let mut turn = 0;
            loop {
                let size = sizes[turn % sizes.len()];
                turn += 1;
                let mut expected = vec![0xa5; size + 3];
                let mut actual = expected.clone();
                let want = baseline.read_reference(&mut expected[..size]);
                let got = candidate.read(&mut actual[..size]);
                assert_eq!(got.is_err(), want.is_err());
                assert_eq!(got.ok(), want.ok());
                assert_eq!(actual, expected);
                assert_eq!(state(&candidate), state(&baseline));
                if baseline.finished() {
                    assert!(candidate.finished());
                    let end = candidate.read(&mut [0; 8]);
                    assert_eq!(end.is_err(), expected_crc != crc);
                    break;
                }
            }
        }
    }

    #[test]
    fn reconstruction_matches_the_previous_loop_at_every_output_edge() {
        let mut cases = vec![
            vec![b'Q'; 4],
            vec![b'Q'; 3],
            vec![b'A', b'A', b'A', b'A', 0],
            vec![b'A', b'A', b'A', b'A', 255],
            vec![b'A', b'A', b'A', b'A', 0, b'A', b'A', b'A', b'A', 7, b'B'],
            vec![0, 0, 0, 0, 3, 0, 0, 0, 0],
        ];
        for (length, seed) in [(64, 1), (1000, 2), (4099, 3)] {
            cases.push(symbols(length, seed));
        }
        // A run count at the very end of the block and a final fourth byte.
        let mut tail = symbols(777, 4);
        tail.extend([9, 9, 9, 9, 200]);
        cases.push(tail.clone());
        tail.truncate(tail.len() - 1);
        cases.push(tail);
        let sizes: [&[usize]; 6] = [
            &[1],
            &[7],
            &[65_536],
            &[0, 1, 7, 0, 2, 255],
            &[3, 256],
            &[1 << 20],
        ];
        for raw in &cases {
            for randomized in [false, true] {
                for random_count in [RANDOM_NUMBERS[0] - 2, 2] {
                    for sizes in sizes {
                        compare(raw, randomized, random_count, sizes);
                    }
                }
            }
        }
        // Long runs straddling 64 KiB output buffers.
        let mut long = symbols(300_000, 5);
        long.extend([7, 7, 7, 7, 255, 7, 7, 7, 7, 255]);
        for randomized in [false, true] {
            for sizes in [&[65_536][..], &[65_535, 65_537], &[1 << 20]] {
                compare(&long, randomized, RANDOM_NUMBERS[0] - 2, sizes);
            }
        }
    }
}
