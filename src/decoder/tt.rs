//! Inverse BWT links, based on the 7-Zip BZip2 DecodeBlock1 function.
//! SPDX-License-Identifier: LGPL-2.1-or-later
use super::super::tt_storage::{TtBuffer, Word};
use core::cell::Cell;

pub(super) trait LinkWord: Word {
    fn set_low(&mut self, low: u32);
    fn finish(links: TtBuffer<Self>) -> TtBuffer<Self> {
        links
    }
}
impl LinkWord for u32 {
    #[inline]
    fn set_low(&mut self, low: u32) {
        *self = low;
    }
}
impl LinkWord for [u32; 2] {
    #[inline]
    fn set_low(&mut self, low: u32) {
        self[0] = low;
    }
    fn finish(mut links: TtBuffer<Self>) -> TtBuffer<Self> {
        // Low halves are complete before this independent successor pass begins.
        // Only low halves are read; in-place high-half writes are order-independent.
        // Every low half holds a block index above its byte, so clamping to
        // the final index is the identity and removes the per-word check.
        let words: &mut [[u32; 2]] = &mut links;
        let Some(final_index) = words.len().checked_sub(1) else {
            return links;
        };
        for index in 0..words.len() {
            let first = words[index][0];
            words[index][1] = words[((first >> 8) as usize).min(final_index)][0];
        }
        links
    }
}

fn starts(mut counts: [usize; 256]) -> [usize; 256] {
    let mut total = 0;
    for count in &mut counts {
        let end = total + *count;
        *count = total;
        total = end;
    }
    counts
}

#[cfg(test)]
fn prepare_links(mut links: Vec<u32>, length: usize) -> Vec<u32> {
    if links.capacity() < length {
        // Release old storage before growing to preserve the live allocation bound.
        links = Vec::new();
        links.reserve_exact(length);
    }
    links.resize(length, 0);
    links
}

/// Scalar DecodeBlock1 histogram scatter.
/// Counts must be the histogram of last, whose length is codec-bounded.
#[cfg(test)]
fn build_tt_checked(last: &[u8], counts: [usize; 256], links: Vec<u32>) -> Vec<u32> {
    let mut counts = starts(counts);
    let mut links = prepare_links(links, last.len());
    for (index, &byte) in last.iter().enumerate() {
        let slot = &mut counts[byte as usize];
        links[*slot] = ((index as u32) << 8) | u32::from(last[*slot]);
        *slot += 1;
    }
    links
}

// Safe unaligned word comparisons; there are no reads beyond input.
fn repeated_prefix(input: &[u8], byte: u8) -> usize {
    let repeated = u64::from_ne_bytes([byte; 8]);
    let mut length = 0;
    while length + 8 <= input.len() {
        if u64::from_ne_bytes(input[length..length + 8].try_into().unwrap()) != repeated {
            break;
        }
        length += 8;
    }
    while length < input.len() && input[length] == byte {
        length += 1;
    }
    length
}

fn mostly_long_runs(last: &[u8]) -> bool {
    if last.len() < 1024 {
        return false;
    }
    let stride = (last.len() - 8) / 127;
    let mut misses = 0;
    for sample in 0..128 {
        let start = sample * stride;
        let word = u64::from_ne_bytes(last[start..start + 8].try_into().unwrap());
        if word != u64::from_ne_bytes([last[start]; 8]) {
            misses += 1;
            if misses > 8 {
                return false;
            }
        }
    }
    true
}

/// Stable histogram scatter, batching only adjacent equal source bytes.
/// No extra allocation: a repeated byte consumes a contiguous portion of its
/// histogram bucket, in the same index order as the checked scalar loop.
pub(super) fn build_links<W: LinkWord>(
    last: &[u8],
    counts: [usize; 256],
    links: TtBuffer<W>,
) -> TtBuffer<W> {
    // Sampling only selects between equivalent stable scatter kernels. A false
    // prediction changes cost, never the decoded bytes or allocation bound.
    if !mostly_long_runs(last) {
        let mut counts = starts(counts);
        let mut links = links.resized(last.len());
        let words: &mut [W] = &mut links[..last.len()];
        if let Some(final_index) = last.len().checked_sub(1) {
            // Both slices are bound once to the same length. Starts from the
            // histogram of last never exceed the final index, so clamping is
            // the identity for parser blocks; it is branch-free and lets the
            // compiler drop both per-byte bounds checks. An inconsistent
            // histogram still stays in bounds, overwriting the final slot.
            // The bucket counter is advanced unclamped, so the chain carried
            // through a repeated bucket stays load, increment, store; the
            // clamp only feeds this iteration's accesses.
            let slots = Cell::from_mut(&mut counts).as_array_of_cells().each_ref();
            let mut scatter = |slot: &Cell<usize>, index: usize| {
                let start = slot.get();
                slot.set(start + 1);
                let destination = start.min(final_index);
                words[destination].set_low(((index as u32) << 8) | u32::from(last[destination]));
            };
            // Like DecodeBlock1, each counter is loaded and stored through
            // one plain address register: slots come from the table of
            // counter references one pair ahead, so the scatter chain never
            // waits on that lookup. Two positions per step keep the clamp a
            // conditional move; equal buckets within a pair share one cell
            // and update it in order. No byte is read beyond last.
            if let [first, second, ahead @ ..] = last {
                let mut pairs = ahead.chunks_exact(2);
                let mut current = slots[usize::from(*first)];
                let mut successor = slots[usize::from(*second)];
                let mut index = 0;
                for pair in pairs.by_ref() {
                    scatter(current, index);
                    scatter(successor, index + 1);
                    current = slots[usize::from(pair[0])];
                    successor = slots[usize::from(pair[1])];
                    index += 2;
                }
                scatter(current, index);
                scatter(successor, index + 1);
                if let [byte] = pairs.remainder() {
                    scatter(slots[usize::from(*byte)], index + 2);
                }
            } else {
                scatter(slots[usize::from(last[0])], 0);
            }
        }
        return W::finish(links);
    }
    let mut counts = starts(counts);
    let mut links = links.resized(last.len());
    let words: &mut [W] = &mut links;
    let mut index = 0;
    while index < last.len() {
        let byte = last[index];
        let destination = counts[byte as usize];
        // The ordinary scatter remains the fast path for nonrepeating input.
        if index + 1 == last.len() || last[index + 1] != byte {
            counts[byte as usize] = destination + 1;
            words[destination].set_low(((index as u32) << 8) | u32::from(last[destination]));
            index += 1;
            continue;
        }
        let run = repeated_prefix(&last[index..], byte);
        let end = destination + run;
        counts[byte as usize] = end;
        // Bounds are established once for the complete destination run. The
        // independent sequential byte loads and index induction can vectorize.
        for (offset, (link, &low)) in words[destination..end]
            .iter_mut()
            .zip(&last[destination..end])
            .enumerate()
        {
            link.set_low((((index + offset) as u32) << 8) | u32::from(low));
        }
        index += run;
    }
    W::finish(links)
}

#[cfg(test)]
mod tests {
    use super::{build_links, build_tt_checked};
    fn counts(input: &[u8]) -> [usize; 256] {
        let mut counts = [0; 256];
        for &byte in input {
            counts[byte as usize] += 1;
        }
        counts
    }
    #[test]
    fn sampler_size_cutoff_and_eight_miss_threshold_are_exact() {
        for length in [1023, 1024, 1025] {
            assert_eq!(super::mostly_long_runs(&vec![42; length]), length >= 1024);
            assert!(!super::mostly_long_runs(
                &(0..length).map(|i| (i % 2) as u8).collect::<Vec<_>>()
            ));
        }
        // At 1024/1025 the stride is eight: sample windows do not overlap.
        for length in [1024, 1025] {
            for misses in [8, 9] {
                let mut input = vec![42; length];
                for sample in 0..misses {
                    input[sample * 8 + 3] = 17;
                }
                assert_eq!(super::mostly_long_runs(&input), misses == 8);
                compare(&input);
            }
        }
    }
    fn compare(input: &[u8]) {
        let histogram = counts(input);
        let expected = build_tt_checked(input, histogram, Vec::new());
        for old in [
            Vec::new(),
            vec![[0xDEAD_BEEFu32, 0xBAD0_CAFE]; input.len() + 17],
            vec![[0x1234_5678u32, 0x9ABC_DEF0]; input.len() / 2],
        ] {
            let paired = build_links(input, histogram, old.into());
            assert_eq!(paired.len(), expected.len());
            for (index, &low) in expected.iter().enumerate() {
                assert_eq!(paired[index], [low, expected[(low >> 8) as usize]]);
            }
        }
        let actual = build_links(input, histogram, Vec::<u32>::new().into());
        assert_eq!(actual.len(), expected.len());
        assert!(
            actual[..] == expected[..],
            "length {}, first differing TT word {:?}",
            input.len(),
            actual.iter().zip(&expected).position(|(a, b)| a != b)
        );
        // Reuse a longer, dirty allocation, then a too-small allocation.
        for old in [
            vec![0xDEADBEEFu32; input.len() + 17],
            vec![0x12345678u32; input.len() / 2],
        ] {
            let actual = build_links(input, histogram, old.into());
            assert!(
                actual[..] == expected[..],
                "reused TT differs at length {}",
                input.len()
            );
        }
    }
    #[test]
    fn bounded_both_word_scatter_matches_checked_low_and_successor_fields() {
        for length in [0, 1, 2, 7, 8, 9, 33, 257, 1024, 1025] {
            compare(&vec![42; length]);
            compare(
                &(0..length)
                    .map(|index| (index % 251) as u8)
                    .collect::<Vec<_>>(),
            );
        }
    }
    #[test]
    fn every_tt_word_matches_checked_for_runs_periods_random_and_edges() {
        let mut state = 0x12345678u32;
        let lengths = [
            0, 1, 2, 3, 4, 7, 8, 9, 15, 16, 17, 31, 32, 33, 255, 256, 257, 4095, 4096, 4097, 65535,
            65536, 65537, 899999, 900000,
        ];
        for length in lengths {
            compare(&vec![42; length]);
            compare(&(0..length).map(|i| (i % 256) as u8).collect::<Vec<_>>());
            compare(&(0..length).map(|i| (i % 2) as u8).collect::<Vec<_>>());
            for run in [2, 7, 8, 9, 37, 259, 4096] {
                compare(
                    &(0..length)
                        .map(|i| ((i / run) % 251) as u8)
                        .collect::<Vec<_>>(),
                );
            }
            let random: Vec<_> = (0..length)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect();
            compare(&random);
        }
    }
    #[test]
    fn clamped_scatter_matches_checked_at_bucket_and_block_extremes() {
        // Lengths 1 and 2 make the final index 0 and 1; short inputs stay on
        // the ordinary scatter whatever their run structure.
        for input in [&[0u8][..], &[255], &[7, 7], &[0, 255], &[255, 0]] {
            compare(input);
        }
        for length in [3, 255, 256, 257, 1023, 1024, 4097] {
            for byte in [0, 128, 255] {
                compare(&vec![byte; length]);
            }
            compare(&(0..length).map(|i| [0, 255][i % 2]).collect::<Vec<_>>());
            compare(&(0..length).map(|i| [255, 0][i % 2]).collect::<Vec<_>>());
            // Ascending and descending full alphabets fill every bucket.
            compare(&(0..length).map(|i| (i % 256) as u8).collect::<Vec<_>>());
            compare(
                &(0..length)
                    .map(|i| 255 - (i % 256) as u8)
                    .collect::<Vec<_>>(),
            );
        }
        // A full block whose histogram is dominated by the last bucket while
        // every other byte still occurs, so the ordinary path is selected.
        let mut state = 0x9E37_79B9u32;
        let skewed: Vec<_> = (0..900_000)
            .map(|index| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                if index < 256 {
                    index as u8
                } else if state.is_multiple_of(8) {
                    (state >> 24) as u8
                } else {
                    255
                }
            })
            .collect();
        assert!(!super::mostly_long_runs(&skewed));
        let histogram = counts(&skewed);
        assert!(histogram.iter().all(|&count| count > 0));
        assert!(histogram[255] > 700_000);
        compare(&skewed);
        let mut low_skew = skewed.clone();
        for byte in &mut low_skew {
            *byte = 255 - *byte;
        }
        assert!(!super::mostly_long_runs(&low_skew));
        compare(&low_skew);
    }
    #[test]
    fn scanned_runs_stop_at_every_word_and_tail_boundary() {
        for alignment in 0..8 {
            for length in 0..280 {
                let mut input = vec![42; alignment + length + 17];
                input[alignment + length] = 17;
                assert_eq!(super::repeated_prefix(&input[alignment..], 42), length);
                assert_eq!(
                    super::repeated_prefix(&input[alignment..alignment + length], 42),
                    length
                );
            }
        }
    }
}
