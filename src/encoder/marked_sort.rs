//! Reusable-storage translation of the public-domain BlockSort / SortGroup.
//! Reference: <https://github.com/ip7z/7zip/blob/9128b80e3a471108678e2b3ec3c985861c8d7b0f/C/BwtSort.c>.
//! Changes: checked typed phases and the existing rotated-rank representation.

const INDEX_BITS: u32 = 20;
const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
#[cfg(test)]
const EXTRA_BITS: u32 = 10;
#[cfg(test)]
const EXTRA_MASK: u32 = (1 << EXTRA_BITS) - 1;
#[cfg(test)]
const UNFINISHED: u32 = 1 << 31;
#[cfg(test)]
const EXTENDED: u32 = 1 << 30;
#[cfg(test)]
const HASH_WORDS: usize = 65536;

#[derive(Clone, Copy)]
pub(super) struct OriginalIndex(u32);

impl OriginalIndex {
    pub(super) fn checked(index: usize, size: usize) -> Self {
        assert!(index < size && size <= 900_000);
        Self(index as u32)
    }

    pub(super) fn get(self) -> usize {
        self.0 as usize
    }

    pub(super) fn unmarked(word: u32) -> Self {
        debug_assert!(word & !INDEX_MASK == 0);
        Self(word)
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct MarkedIndex(u32);

#[cfg(test)]
impl MarkedIndex {
    fn original(self) -> OriginalIndex {
        // All original indices were checked at initialization. Markers modify
        // only the high bits; decode BEFORE any logical-coordinate arithmetic.
        OriginalIndex(self.0 & INDEX_MASK)
    }
}

// These high bits are rank data, not group markers. Only this phase uses the
// variable ref_bits split, and it must not pass through MarkedIndex::original.
#[cfg(test)]
#[derive(Clone, Copy)]
struct PackedRank(u32);

#[cfg(test)]
impl PackedRank {
    fn new(rank: u32, index: usize, ref_bits: u32) -> Self {
        assert!(index < 1usize << ref_bits);
        debug_assert!(rank <= u32::MAX >> ref_bits);
        Self((rank << ref_bits) | index as u32)
    }

    fn rank(self, ref_bits: u32) -> u32 {
        self.0 >> ref_bits
    }

    fn index(self, ref_bits: u32) -> usize {
        (self.0 & ((1 << ref_bits) - 1)) as usize
    }
}

pub(super) struct Ranks<'a, const CAPACITY: usize> {
    values: &'a mut [u32; CAPACITY],
    len: usize,
    shift: usize,
}

impl<const CAPACITY: usize> Ranks<'_, CAPACITY> {
    pub(super) fn new(values: &mut [u32; CAPACITY], len: usize) -> Ranks<'_, CAPACITY> {
        assert!(len > 0 && len <= CAPACITY);
        Ranks {
            values,
            len,
            shift: 0,
        }
    }
    fn storage_position(&self, index: OriginalIndex) -> usize {
        let index = index.get();
        if index >= self.shift {
            index - self.shift
        } else {
            index + self.len - self.shift
        }
    }

    pub(super) fn write(&mut self, index: OriginalIndex, rank: u32) {
        let physical = self.storage_position(index);
        let slot = if CAPACITY.is_power_of_two() {
            physical & (CAPACITY - 1)
        } else {
            physical.min(CAPACITY - 1)
        };
        self.values[slot] = rank;
    }

    pub(super) fn key(&self, index: OriginalIndex) -> u32 {
        // OriginalIndex and rotated physical coordinates are below logical
        // len <= CAPACITY; clamp only certifies that bound to the compiler.
        let slot = if CAPACITY.is_power_of_two() {
            index.get() & (CAPACITY - 1)
        } else {
            index.get().min(CAPACITY - 1)
        };
        self.values[slot]
    }

    pub(super) fn advance(&mut self, width: usize) {
        assert!(width >= self.shift && width < self.len);
        self.values[..self.len].rotate_left(width - self.shift);
        self.shift = width;
    }
}

// SetGroupSize / SetFinishedGroupSize. Call only on original-index words.
#[cfg(test)]
fn mark_group(order: &mut [u32], size: usize, unfinished: bool) {
    assert!(size > 0 && size <= order.len());
    let encoded = (size - 1) as u32;
    if unfinished && encoded == 0 {
        return;
    }
    order[0] |= ((encoded & EXTRA_MASK) << INDEX_BITS) | if unfinished { UNFINISHED } else { 0 };
    if encoded >= 1 << EXTRA_BITS {
        order[0] |= EXTENDED;
        order[1] |= (encoded >> EXTRA_BITS) << INDEX_BITS;
    }
}

#[cfg(test)]
fn decode_group(order: &mut [u32]) -> (usize, bool) {
    let head = MarkedIndex(order[0]);
    let mut encoded = (head.0 & !(UNFINISHED | EXTENDED)) >> INDEX_BITS;
    if head.0 & EXTENDED != 0 {
        encoded += (order[1] >> INDEX_BITS) << EXTRA_BITS;
        order[1] = MarkedIndex(order[1]).original().0;
    }
    order[0] = head.original().0;
    let size = encoded as usize + 1;
    assert!(size <= order.len());
    (size, head.0 & UNFINISHED == 0)
}

// SortGroup: same partition/traversal and packed-small-group ordering, with
// unresolved ranges encoded in order's high bits instead of separate vectors.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn refine<const CAPACITY: usize>(
    order: &mut [u32],
    ranks: &mut Ranks<'_, CAPACITY>,
    offset: usize,
    ref_bits: u32,
    mut left: usize,
    mut range: usize,
    temporary: &mut [u32],
) -> bool {
    if order.len() <= 1 {
        return false;
    }
    if order.len() <= 1usize << ref_bits && order.len() <= range {
        let temporary = &mut temporary[..order.len()];
        let first = ranks.key(MarkedIndex(order[0]).original());
        let mut different = false;
        for (i, &word) in order.iter().enumerate() {
            let rank = ranks.key(MarkedIndex(word).original());
            different |= rank != first;
            temporary[i] = PackedRank::new(rank, i, ref_bits).0;
        }
        if !different {
            mark_group(order, order.len(), true);
            return true;
        }
        temporary.sort_unstable();
        let mut start = 0;
        let mut previous = PackedRank(temporary[0]).rank(ref_bits);
        let mut unresolved = false;
        for i in 0..temporary.len() {
            let packed = PackedRank(temporary[i]);
            let rank = packed.rank(ref_bits);
            if rank != previous {
                let size = i - start;
                unresolved |= size > 1;
                mark_group(&mut temporary[start..], size, true);
                start = i;
                previous = rank;
            }
            let position = MarkedIndex(order[packed.index(ref_bits)]).original();
            temporary[i] = position.0;
            ranks.write(position, (offset + start) as u32);
        }
        let size = temporary.len() - start;
        unresolved |= size > 1;
        mark_group(&mut temporary[start..], size, true);
        order.copy_from_slice(temporary);
        return unresolved;
    }
    let first = ranks.key(MarkedIndex(order[0]).original());
    if order
        .iter()
        .all(|&word| ranks.key(MarkedIndex(word).original()) == first)
    {
        mark_group(order, order.len(), true);
        return true;
    }
    let (split, mid) = loop {
        if range <= 1 {
            mark_group(order, order.len(), true);
            return true;
        }
        let mid = left + range.div_ceil(2);
        let mut low = 0;
        let mut remaining = &mut order[..];
        while !remaining.is_empty() {
            if (ranks.key(MarkedIndex(remaining[0]).original()) as usize) < mid {
                low += 1;
                remaining = &mut remaining[1..];
            } else {
                loop {
                    let last = remaining.len() - 1;
                    if last == 0 {
                        remaining = &mut remaining[..0];
                        break;
                    }
                    if (ranks.key(MarkedIndex(remaining[last]).original()) as usize) < mid {
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
    for &word in &order[split..] {
        ranks.write(MarkedIndex(word).original(), (offset + split) as u32);
    }
    let (a, b) = order.split_at_mut(split);
    let a_unresolved = refine(a, ranks, offset, ref_bits, left, mid - left, temporary);
    let b_unresolved = refine(
        b,
        ranks,
        offset + split,
        ref_bits,
        mid,
        range - (mid - left),
        temporary,
    );
    a_unresolved | b_unresolved
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct Workspace {
    // First n: order; next65536: hash then small-sort scratch; final CAPACITY:
    // rotated ranks. Length records initialization, capacity survives reuse.
    storage: Vec<u32>,
}

#[cfg(test)]
impl Workspace {
    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    #[cfg(test)]
    fn transform(&mut self, input: &[u8]) -> (Vec<u8>, usize) {
        let (order, origin) = self.sort(input).unwrap();
        let transformed = order
            .iter()
            .map(|&position| {
                let position = position as usize;
                input[if position == 0 {
                    input.len() - 1
                } else {
                    position - 1
                }]
            })
            .collect();
        (transformed, origin)
    }

    pub(super) fn sort(&mut self, input: &[u8]) -> std::io::Result<(&[u32], usize)> {
        assert!(!input.is_empty() && input.len() <= 900_000);
        match input.len() {
            1..=256 => self.sort_for::<256>(input),
            257..=4096 => self.sort_for::<4096>(input),
            4097..=65536 => self.sort_for::<65536>(input),
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
        let (temporary, values) = rest.split_at_mut(HASH_WORDS);
        temporary.fill(0);
        let mut ranks = Ranks::<CAPACITY> {
            values: values.try_into().unwrap(),
            len: size,
            shift: 0,
        };
        let hash = |i: usize| {
            (usize::from(input[i]) << 8) | usize::from(input[if i + 1 == size { 0 } else { i + 1 }])
        };
        for i in 0..size {
            temporary[hash(i)] += 1;
        }
        let mut total = 0;
        for count in temporary.iter_mut() {
            let size = *count;
            *count = total;
            total += size;
        }
        for i in 0..size {
            ranks.values[i] = temporary[hash(i)];
        }
        for i in 0..size {
            let position = OriginalIndex::checked(i, size);
            let slot = &mut temporary[hash(i)];
            order[*slot as usize] = position.0;
            *slot += 1;
        }
        let mut previous = 0;
        for &end in temporary.iter() {
            let end = end as usize;
            if end > previous {
                mark_group(&mut order[previous..], end - previous, true);
                previous = end;
            }
        }
        let ref_bits = (32 - (usize::BITS - (size - 1).leading_zeros())).min(12);
        let mut width = 2;
        loop {
            if width < size {
                ranks.advance(width);
            }
            let mut finished = 0;
            let mut offset = 0;
            let mut unresolved = false;
            while offset < size {
                let (count, was_finished) = decode_group(&mut order[offset..]);
                if was_finished || count == 1 {
                    let first = offset - finished;
                    order[first] = MarkedIndex(order[first]).original().0;
                    if finished > 1 {
                        order[first + 1] = MarkedIndex(order[first + 1]).original().0;
                    }
                    finished += count;
                    mark_group(&mut order[first..], finished, false);
                } else {
                    finished = 0;
                    if width >= size {
                        // Reference's final tie pass assigns exact ranks; it
                        // does not reorder rotations that remain identical.
                        for (i, &word) in order[offset..offset + count].iter().enumerate() {
                            ranks.write(MarkedIndex(word).original(), (offset + i) as u32);
                        }
                    } else {
                        unresolved |= refine(
                            &mut order[offset..offset + count],
                            &mut ranks,
                            offset,
                            ref_bits,
                            0,
                            size,
                            temporary,
                        );
                    }
                }
                offset += count;
            }
            if !unresolved {
                break;
            }
            width *= 2;
        }
        // Decode every original position before origin or predecessor access;
        // metadata may span two words and need not begin at the origin word.
        for word in order.iter_mut() {
            *word = MarkedIndex(*word).original().0;
        }
        let origin = order.iter().position(|&position| position == 0).unwrap();
        Ok((order, origin))
    }
}

#[cfg(test)]
mod tests {
    use super::{EXTENDED, INDEX_MASK, MarkedIndex, OriginalIndex, PackedRank, Ranks, Workspace};

    #[test]
    fn marker_coordinates_decode_before_logical_wrap_and_packed_rank_split() {
        let mut values = [0u32; 16];
        let mut ranks = Ranks {
            values: &mut values,
            len: 11,
            shift: 0,
        };
        for i in 0..11 {
            ranks.write(OriginalIndex::checked(i, 11), i as u32);
        }
        ranks.advance(7);
        for flags in [0, 0x8000_0000, 0xc3f0_0000, EXTENDED] {
            for index in 0..11 {
                let original = MarkedIndex(flags | index).original();
                assert_eq!(
                    ranks.storage_position(original),
                    (index as usize + 11 - 7) % 11
                );
                ranks.write(original, 100 + index);
                assert_eq!(ranks.values[ranks.storage_position(original)], 100 + index);
            }
        }
        let packed = PackedRank::new(899_999, 4095, 12);
        assert!(packed.0 & !INDEX_MASK != 0);
        assert_eq!(packed.rank(12), 899_999);
        assert_eq!(packed.index(12), 4095);
    }

    #[test]
    fn in_order_metadata_round_trips_both_encodings_and_singletons() {
        for size in [1, 2, 127, 128, 129, 1024, 1025, 4096, 65536, 900000] {
            for unfinished in [false, true] {
                let mut order: Vec<_> = (0..size as u32).collect();
                super::mark_group(&mut order, size, unfinished);
                let (actual, finished) = super::decode_group(&mut order);
                assert_eq!(actual, size);
                assert_eq!(finished, !unfinished || size == 1);
                assert_eq!(order, (0..size as u32).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn reusable_marker_sort_preserves_bwt_bytes_and_exact_origin() {
        let mut workspace = Workspace::default();
        let mut state = 0x1923_ab5du32;
        for size in (1..96).chain([
            255, 256, 257, 1024, 1025, 4095, 4096, 4097, 65535, 65536, 65537, 900000,
        ]) {
            for period in [1, 2, 7, 251, 0] {
                let input: Vec<_> = (0..size)
                    .map(|i| {
                        if period != 0 {
                            return (i % period) as u8;
                        }
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        state as u8
                    })
                    .collect();
                let expected = super::super::sort::transform(&input);
                let actual = workspace.transform(&input);
                assert_eq!(actual.1, expected.1, "origin size={size},period={period}");
                assert!(actual.0 == expected.0, "BWT size={size},period={period}");
            }
        }
        let capacity = workspace.capacity();
        assert!(capacity >= 900_000 + super::HASH_WORDS + 1_048_576);
        for input in [b"a".as_slice(), b"abracadabra", b"xyxyxyxyxyxyx"] {
            assert_eq!(
                workspace.transform(input),
                super::super::sort::transform(input)
            );
            assert_eq!(workspace.capacity(), capacity);
        }
    }

    #[test]
    fn marker_sort_also_matches_an_independent_rotation_oracle() {
        let mut workspace = Workspace::default();
        for size in 1..160 {
            let input: Vec<_> = (0..size)
                .map(|i| ((i * i * 37 + i * 17 + size) % 251) as u8)
                .collect();
            let mut rotations: Vec<_> = (0..size).collect();
            rotations.sort_by(|&a, &b| {
                (0..size)
                    .map(|i| input[(a + i) % size])
                    .cmp((0..size).map(|i| input[(b + i) % size]))
            });
            let expected: Vec<_> = rotations
                .iter()
                .map(|&i| input[if i == 0 { size - 1 } else { i - 1 }])
                .collect();
            let (actual, origin) = workspace.transform(&input);
            assert_eq!(actual, expected);
            assert!((0..size).all(|i| input[(rotations[origin] + i) % size] == input[i]));
            let (order, order_origin) = workspace.sort(&input).unwrap();
            let mut permutation = order.to_vec();
            permutation.sort_unstable();
            assert_eq!(permutation, (0..size as u32).collect::<Vec<_>>());
            assert_eq!(order[order_origin], 0);
            assert!(order.windows(2).all(|pair| {
                (0..size)
                    .map(|i| input[(pair[0] as usize + i) % size])
                    .cmp((0..size).map(|i| input[(pair[1] as usize + i) % size]))
                    .is_le()
            }));
        }
    }

    #[test]
    fn both_small_group_cutoffs_preserve_unique_rank_partition_order() {
        for ref_bits in [2, 4] {
            for range in [8, 16] {
                let mut values = [0u32; 16];
                for (i, slot) in values[..8].iter_mut().enumerate() {
                    *slot = (7 - i) as u32;
                }
                let mut ranks = Ranks {
                    values: &mut values,
                    // Keys occupy physical0..8; writes for these original
                    // positions occupy8..16, as in a disjoint shifted group.
                    len: 16,
                    shift: 8,
                };
                let mut order: Vec<_> = (0..8).collect();
                let mut scratch = [0u32; 16];
                assert!(!super::refine(
                    &mut order,
                    &mut ranks,
                    0,
                    ref_bits,
                    0,
                    range,
                    &mut scratch
                ));
                let decoded: Vec<_> = order
                    .into_iter()
                    .map(|word| MarkedIndex(word).original().0)
                    .collect();
                assert_eq!(decoded, (0..8).rev().collect::<Vec<_>>());
            }
        }
    }
}
