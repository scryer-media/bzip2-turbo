//! BlockSort / SortGroup's public-domain external-flags storage mode.
//! Reference: <https://github.com/ip7z/7zip/blob/9128b80e3a471108678e2b3ec3c985861c8d7b0f/C/BwtSort.c>.
//! Changes: word-bounded flag discovery and reuse of the expired hash region.

use super::marked_sort::{OriginalIndex, Ranks};

const HASH_WORDS: usize = 65536;
const TEMP_WORDS: usize = 4096;

struct Flags<'a> {
    words: &'a mut [u32],
    len: usize,
}

impl Flags<'_> {
    fn clear(&mut self, index: usize) {
        assert!(index < self.len);
        self.words[index / 32] &= !(1 << (index & 31));
    }

    fn next(&self, start: usize, set: bool) -> usize {
        if start >= self.len {
            return self.len;
        }
        let mut index = start / 32;
        let mut word = if set {
            self.words[index]
        } else {
            !self.words[index]
        };
        word &= u32::MAX << (start & 31);
        loop {
            if word != 0 {
                return (index * 32 + word.trailing_zeros() as usize).min(self.len);
            }
            index += 1;
            if index == self.words.len() {
                return self.len;
            }
            word = if set {
                self.words[index]
            } else {
                !self.words[index]
            };
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn refine<const CAPACITY: usize>(
    order: &mut [u32],
    ranks: &mut Ranks<'_, CAPACITY>,
    offset: usize,
    ref_bits: u32,
    mut left: usize,
    mut range: usize,
    flags: &mut Flags<'_>,
    temporary: &mut [u32],
) -> bool {
    if order.len() <= 1 {
        return false;
    }
    if order.len() <= 1usize << ref_bits && order.len() <= range {
        let temporary = &mut temporary[..order.len()];
        let first = ranks.key(OriginalIndex::unmarked(order[0]));
        let mut different = false;
        for (i, &word) in order.iter().enumerate() {
            let rank = ranks.key(OriginalIndex::unmarked(word));
            different |= rank != first;
            debug_assert!(rank <= u32::MAX >> ref_bits);
            // This is a packed rank/local-index phase, never group metadata.
            temporary[i] = (rank << ref_bits) | i as u32;
        }
        if !different {
            return true;
        }
        temporary.sort_unstable();
        let mask = (1 << ref_bits) - 1;
        let mut start = 0;
        let mut previous = temporary[0] >> ref_bits;
        let mut unresolved = false;
        for (i, slot) in temporary.iter_mut().enumerate() {
            let packed = *slot;
            let rank = packed >> ref_bits;
            if rank != previous {
                flags.clear(offset + i - 1);
                unresolved |= i - start > 1;
                start = i;
                previous = rank;
            }
            let position = OriginalIndex::unmarked(order[(packed & mask) as usize]);
            *slot = position.get() as u32;
            ranks.write(position, (offset + start) as u32);
        }
        unresolved |= order.len() - start > 1;
        order.copy_from_slice(temporary);
        return unresolved;
    }
    let first = ranks.key(OriginalIndex::unmarked(order[0]));
    if order
        .iter()
        .all(|&word| ranks.key(OriginalIndex::unmarked(word)) == first)
    {
        return true;
    }
    let (split, mid) = loop {
        if range <= 1 {
            return true;
        }
        let mid = left + range.div_ceil(2);
        let mut low = 0;
        let mut remaining = &mut order[..];
        while !remaining.is_empty() {
            if (ranks.key(OriginalIndex::unmarked(remaining[0])) as usize) < mid {
                low += 1;
                remaining = &mut remaining[1..];
            } else {
                loop {
                    let last = remaining.len() - 1;
                    if last == 0 {
                        remaining = &mut remaining[..0];
                        break;
                    }
                    if (ranks.key(OriginalIndex::unmarked(remaining[last])) as usize) < mid {
                        remaining.swap(0, last);
                        low += 1;
                        remaining = &mut remaining[1..last];
                        break;
                    }
                    remaining = &mut remaining[..last];
                }
            }
        }
        if low == 0 {
            range -= mid - left;
            left = mid;
        } else if low == order.len() {
            range = mid - left;
        } else {
            break (low, mid);
        }
    };
    flags.clear(offset + split - 1);
    for &word in &order[split..] {
        ranks.write(OriginalIndex::unmarked(word), (offset + split) as u32);
    }
    let (a, b) = order.split_at_mut(split);
    let a = refine(
        a,
        ranks,
        offset,
        ref_bits,
        left,
        mid - left,
        flags,
        temporary,
    );
    let b = refine(
        b,
        ranks,
        offset + split,
        ref_bits,
        mid,
        range - (mid - left),
        flags,
        temporary,
    );
    a | b
}

#[derive(Default)]
pub(super) struct Workspace {
    storage: Vec<u32>,
}

impl Workspace {
    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    pub(super) fn sort(&mut self, input: &[u8]) -> std::io::Result<(&[u32], usize)> {
        assert!(!input.is_empty() && input.len() <= 900_000);
        match input.len() {
            1..=256 => self.sort_for::<256>(input),
            257..=4096 => self.sort_for::<4096>(input),
            4097..=65536 => self.sort_for::<65536>(input),
            65537..=131072 => self.sort_for::<131072>(input),
            131073..=262144 => self.sort_for::<262144>(input),
            262145..=524288 => self.sort_for::<524288>(input),
            _ if cfg!(all(target_os = "macos", target_arch = "aarch64")) => {
                self.sort_for::<900000>(input)
            }
            _ => self.sort_for::<1048576>(input),
        }
    }

    fn sort_for<const CAPACITY: usize>(
        &mut self,
        input: &[u8],
    ) -> std::io::Result<(&[u32], usize)> {
        let size = input.len();
        let needed = size + HASH_WORDS + CAPACITY;
        if self.storage.capacity() < needed {
            self.storage
                .try_reserve_exact(needed - self.storage.len())
                .map_err(std::io::Error::other)?;
        }
        if self.storage.len() < needed {
            self.storage.resize(needed, 0);
        }
        let (order, rest) = self.storage[..needed].split_at_mut(size);
        let (scratch, values) = rest.split_at_mut(HASH_WORDS);
        let values: &mut [u32; CAPACITY] = values.try_into().unwrap();
        scratch.fill(0);
        let hash = |i: usize| {
            (usize::from(input[i]) << 8) | usize::from(input[if i + 1 == size { 0 } else { i + 1 }])
        };
        for i in 0..size {
            scratch[hash(i)] += 1;
        }
        let mut total = 0;
        for count in scratch.iter_mut() {
            let size = *count;
            *count = total;
            total += size;
        }
        for i in 0..size {
            values[i] = scratch[hash(i)];
        }
        for i in 0..size {
            let position = OriginalIndex::checked(i, size);
            let slot = &mut scratch[hash(i)];
            order[*slot as usize] = position.get() as u32;
            *slot += 1;
        }
        // Every hash read is complete before repurposing scratch as flags.
        // Initial ranks retain the group identity independently of counters.
        let flag_words = size.div_ceil(32);
        assert!(flag_words + TEMP_WORDS <= HASH_WORDS);
        let (bitmap, temporary) = scratch.split_at_mut(flag_words);
        let temporary = &mut temporary[..TEMP_WORDS];
        bitmap.fill(u32::MAX);
        if size & 31 != 0 {
            *bitmap.last_mut().unwrap() &= (1u32 << (size & 31)) - 1;
        }
        let mut flags = Flags {
            words: bitmap,
            len: size,
        };
        for i in 0..size - 1 {
            if values[order[i] as usize] != values[order[i + 1] as usize] {
                flags.clear(i);
            }
        }
        flags.clear(size - 1);
        let mut ranks = Ranks::new(values, size);
        let ref_bits = (32 - (usize::BITS - (size - 1).leading_zeros())).min(12);
        let mut width = 2;
        loop {
            if width < size {
                ranks.advance(width);
            }
            let mut offset = flags.next(0, true);
            let mut unresolved = false;
            while offset < size {
                let end = flags.next(offset, false);
                let count = end + 1 - offset;
                assert!(count > 1 && end < size);
                if width >= size {
                    for (i, &word) in order[offset..=end].iter().enumerate() {
                        ranks.write(OriginalIndex::unmarked(word), (offset + i) as u32);
                    }
                } else {
                    unresolved |= refine(
                        &mut order[offset..=end],
                        &mut ranks,
                        offset,
                        ref_bits,
                        0,
                        size,
                        &mut flags,
                        temporary,
                    );
                }
                offset = flags.next(end + 1, true);
            }
            if !unresolved {
                break;
            }
            width *= 2;
        }
        let origin = order.iter().position(|&position| position == 0).unwrap();
        Ok((order, origin))
    }
}

#[cfg(test)]
mod tests {
    use super::{Flags, Workspace};

    #[test]
    fn word_discovery_matches_bitwise_scans_at_every_alignment_and_tail() {
        let mut state = 0x1923_ab5du32;
        for len in 1usize..300 {
            let mut words: Vec<_> = (0..len.div_ceil(32))
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state
                })
                .collect();
            if len & 31 != 0 {
                *words.last_mut().unwrap() &= (1 << (len & 31)) - 1;
            }
            let flags = Flags {
                words: &mut words,
                len,
            };
            for start in 0..=len {
                for set in [false, true] {
                    let expected = (start..len)
                        .find(|&i| ((flags.words[i / 32] >> (i & 31)) & 1 != 0) == set)
                        .unwrap_or(len);
                    assert_eq!(flags.next(start, set), expected);
                }
            }
        }
        let mut words = [0u32; 8];
        let flags = Flags {
            words: &mut words,
            len: 255,
        };
        assert_eq!(flags.next(0, true), 255);
        assert_eq!(flags.next(0, false), 0);
    }

    #[test]
    fn flags_preserve_full_bwt_and_origin_across_capacities_periods_and_reuse() {
        let mut flags = Workspace::default();
        let mut marked = super::super::marked_sort::Workspace::default();
        let mut state = 0x1923_ab5du32;
        for size in (1..128).chain([
            255, 256, 257, 1024, 1025, 4095, 4096, 4097, 65535, 65536, 65537, 900000,
        ]) {
            for period in [1, 2, 7, 251, 0] {
                let input: Vec<_> = (0..size)
                    .map(|i| {
                        if period != 0 {
                            (i % period) as u8
                        } else {
                            state ^= state << 13;
                            state ^= state >> 17;
                            state ^= state << 5;
                            state as u8
                        }
                    })
                    .collect();
                let (actual, origin) = flags.sort(&input).unwrap();
                let (expected, expected_origin) = marked.sort(&input).unwrap();
                assert_eq!(origin, expected_origin, "size={size},period={period}");
                assert_eq!(
                    actual, expected,
                    "rotation order size={size},period={period}"
                );
                let expected = super::super::sort::transform(&input);
                let bwt: Vec<_> = actual
                    .iter()
                    .map(|&i| input[if i == 0 { size - 1 } else { i as usize - 1 }])
                    .collect();
                assert!(bwt == expected.0, "BWT size={size},period={period}");
                assert_eq!(origin, expected.1);
            }
        }
        let capacity = flags.storage.capacity();
        let initialized = flags.storage.len();
        for input in [b"x".as_slice(), b"small repeated buffer", b"xyxyxyxyx"] {
            assert_eq!(flags.sort(input).unwrap(), marked.sort(input).unwrap());
            assert_eq!(flags.storage.capacity(), capacity);
            assert_eq!(flags.storage.len(), initialized);
        }
    }

    #[test]
    fn finer_rank_dispatch_matches_the_full_size_array_at_each_capacity_edge() {
        let mut full = Workspace::default();
        let mut exact = Workspace::default();
        for size in [
            65536usize, 65537, 99999, 100000, 131071, 131072, 131073, 262143, 262144, 262145,
            299999, 300000, 500000, 524287, 524288, 524289, 700000, 900000,
        ] {
            let input: Vec<_> = (0..size)
                .map(|i| ((i * 17 + i / 251) % 251) as u8)
                .collect();
            let mut finer = Workspace::default();
            let (actual, origin) = finer.sort(&input).unwrap();
            let (expected, expected_origin) = full.sort_for::<1048576>(&input).unwrap();
            assert_eq!(origin, expected_origin, "origin at n={size}");
            assert!(actual == expected, "rotation order at n={size}");
            // Both representations are checked on every platform, independent
            // of which largest-capacity dispatch has native measurements.
            let (clamped, clamped_origin) = exact.sort_for::<900000>(&input).unwrap();
            assert_eq!(
                clamped_origin, expected_origin,
                "clamped origin at n={size}"
            );
            assert!(clamped == expected, "clamped rotation order at n={size}");
            let rank_capacity = match size {
                1..=256 => 256,
                257..=4096 => 4096,
                4097..=65536 => 65536,
                65537..=131072 => 131072,
                131073..=262144 => 262144,
                262145..=524288 => 524288,
                _ if cfg!(all(target_os = "macos", target_arch = "aarch64")) => 900000,
                _ => 1048576,
            };
            assert_eq!(
                finer.storage.len(),
                size + super::HASH_WORDS + rank_capacity
            );
            assert!(finer.storage.capacity() >= finer.storage.len());
            let capacity = finer.storage.capacity();
            finer.sort(b"small reused ranks").unwrap();
            assert_eq!(finer.storage.capacity(), capacity);
        }
    }
}
