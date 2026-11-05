//! Safe translation of the public-domain `BlockSort` / `SortGroup` algorithms.
//! Reference: <https://github.com/ip7z/7zip/blob/9128b80e3a471108678e2b3ec3c985861c8d7b0f/C/BwtSort.c>.

// Logical rank R(p) lives at physical slot (p - shift) modulo len. During a
// width-w pass, shift == w, so slot p directly contains R((p + w) modulo len).
// Writes still name original positions: later groups see every live update in
// the same pass. Only the initialized prefix rotates; padding stays untouched.
struct Ranks<const CAPACITY: usize> {
    values: Box<[u32; CAPACITY]>,
    len: usize,
    shift: usize,
}

impl<const CAPACITY: usize> Ranks<CAPACITY> {
    fn storage_position(&self, index: usize) -> usize {
        if index >= self.shift {
            index - self.shift
        } else {
            index + self.len - self.shift
        }
    }

    fn advance(&mut self, width: usize) {
        debug_assert!(width >= self.shift && width < self.len);
        self.values[..self.len].rotate_left(width - self.shift);
        self.shift = width;
    }
}

impl<const CAPACITY: usize> std::ops::Index<usize> for Ranks<CAPACITY> {
    type Output = u32;
    fn index(&self, index: usize) -> &u32 {
        // All positions originate in 0..block length and sorting only permutes
        // them. The mask establishes a constant array bound for safe indexing;
        // it never changes an established position.
        &self.values[self.storage_position(index) & (CAPACITY - 1)]
    }
}

impl<const CAPACITY: usize> std::ops::IndexMut<usize> for Ranks<CAPACITY> {
    fn index_mut(&mut self, index: usize) -> &mut u32 {
        let position = self.storage_position(index);
        &mut self.values[position & (CAPACITY - 1)]
    }
}

fn key<const CAPACITY: usize>(index: u32, ranks: &Ranks<CAPACITY>) -> u32 {
    ranks.values[index as usize & (CAPACITY - 1)]
}

// SortGroup: refine one equal-prefix group, retaining its range-partition order.
#[allow(clippy::too_many_arguments)]
fn refine<const CAPACITY: usize>(
    order: &mut [u32],
    ranks: &mut Ranks<CAPACITY>,
    offset: usize,
    ref_bits: u32,
    mut left: usize,
    mut range: usize,
    groups: &mut Vec<(usize, usize)>,
    temporary: &mut Vec<u32>,
) {
    if order.len() <= 1 {
        return;
    }
    if order.len() <= 1usize << ref_bits && order.len() <= range {
        temporary.clear();
        let initial = key(order[0], ranks);
        let mut different = false;
        for (index, &position) in order.iter().enumerate() {
            let rank = key(position, ranks);
            different |= rank != initial;
            temporary.push((rank << ref_bits) | index as u32);
        }
        if !different {
            groups.push((offset, order.len()));
            return;
        }
        temporary.sort_unstable();
        let mask = (1u32 << ref_bits) - 1;
        let mut start = 0;
        let mut previous = temporary[0] >> ref_bits;
        for (i, slot) in temporary.iter_mut().enumerate() {
            let packed = *slot;
            let rank = packed >> ref_bits;
            if rank != previous {
                if i - start > 1 {
                    groups.push((offset + start, i - start));
                }
                start = i;
                previous = rank;
            }
            let position = order[(packed & mask) as usize];
            *slot = position;
            ranks[position as usize] = (offset + start) as u32;
        }
        if order.len() - start > 1 {
            groups.push((offset + start, order.len() - start));
        }
        order.copy_from_slice(temporary);
        return;
    }
    let first = key(order[0], ranks);
    if order.iter().all(|&index| key(index, ranks) == first) {
        groups.push((offset, order.len()));
        return;
    }
    let (split, mid) = loop {
        if range <= 1 {
            groups.push((offset, order.len()));
            return;
        }
        let mid = left + range.div_ceil(2);
        let mut low = 0;
        let mut remaining = &mut order[..];
        while !remaining.is_empty() {
            if (key(remaining[0], ranks) as usize) < mid {
                low += 1;
                remaining = &mut remaining[1..];
            } else {
                loop {
                    let last = remaining.len() - 1;
                    if last == 0 {
                        remaining = &mut remaining[..0];
                        break;
                    }
                    if (key(remaining[last], ranks) as usize) < mid {
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
    for &index in &order[split..] {
        ranks[index as usize] = (offset + split) as u32;
    }
    let (a, b) = order.split_at_mut(split);
    refine(
        a,
        ranks,
        offset,
        ref_bits,
        left,
        mid - left,
        groups,
        temporary,
    );
    refine(
        b,
        ranks,
        offset + split,
        ref_bits,
        mid,
        range - (mid - left),
        groups,
        temporary,
    );
}

// BlockSort: initialize by two bytes, then double the known prefix length.
pub(super) fn transform(input: &[u8]) -> (Vec<u8>, usize) {
    match input.len() {
        0..=256 => transform_for::<256>(input),
        257..=4096 => transform_for::<4096>(input),
        4097..=65536 => transform_for::<65536>(input),
        _ => transform_for::<1048576>(input),
    }
}

fn transform_for<const CAPACITY: usize>(input: &[u8]) -> (Vec<u8>, usize) {
    let size = input.len();
    let mut counters = vec![0usize; 65536];
    let hash = |i: usize| {
        (usize::from(input[i]) << 8) | usize::from(input[if i + 1 == size { 0 } else { i + 1 }])
    };
    for i in 0..size {
        counters[hash(i)] += 1;
    }
    let mut groups = Vec::new();
    let mut total = 0;
    for count in &mut counters {
        let length = *count;
        *count = total;
        if length > 1 {
            groups.push((total, length));
        }
        total += length;
    }
    let mut ranks: Ranks<CAPACITY> = Ranks {
        values: vec![0u32; CAPACITY].into_boxed_slice().try_into().unwrap(),
        len: size,
        shift: 0,
    };
    for (i, rank) in ranks.values[..size].iter_mut().enumerate() {
        *rank = counters[hash(i)] as u32;
    }
    let mut order = vec![0u32; size];
    for i in 0..size {
        let slot = &mut counters[hash(i)];
        order[*slot] = i as u32;
        *slot += 1;
    }
    let ref_bits = (32 - (usize::BITS - (size - 1).leading_zeros())).min(12);
    let mut width = 2;
    let mut temporary = Vec::new();
    while !groups.is_empty() && width < size {
        ranks.advance(width);
        let mut next = Vec::new();
        for (start, count) in groups {
            refine(
                &mut order[start..start + count],
                &mut ranks,
                start,
                ref_bits,
                0,
                size,
                &mut next,
                &mut temporary,
            );
        }
        groups = next;
        width *= 2;
    }
    let origin = order.iter().position(|&i| i == 0).unwrap();
    let transformed = order
        .into_iter()
        .map(|i| input[if i == 0 { size - 1 } else { i as usize - 1 }])
        .collect();
    (transformed, origin)
}

#[cfg(test)]
mod tests {
    #[test]
    fn rotated_coordinates_preserve_logical_ranks_and_live_writes() {
        let mut ranks = super::Ranks::<16> {
            values: Box::new([0xDEAD_BEEF; 16]),
            len: 11,
            shift: 0,
        };
        let mut expected = [0u32; 11];
        for (position, rank) in expected.iter_mut().enumerate() {
            *rank = 100 + position as u32;
            ranks[position] = *rank;
        }
        for width in [2, 4, 8] {
            ranks.advance(width);
            for position in 0..expected.len() {
                assert_eq!(ranks[position], expected[position]);
                assert_eq!(
                    super::key(position as u32, &ranks),
                    expected[(position + width) % expected.len()]
                );
            }
            for position in [0, 3, 10] {
                expected[position] = 1000 + (width * 16 + position) as u32;
                ranks[position] = expected[position];
                for index in 0..expected.len() {
                    assert_eq!(ranks[index], expected[index]);
                    assert_eq!(
                        super::key(index as u32, &ranks),
                        expected[(index + width) % expected.len()]
                    );
                }
            }
            assert!(ranks.values[11..].iter().all(|&value| value == 0xDEAD_BEEF));
        }
    }

    #[test]
    fn cyclic_sort_matches_a_direct_rotation_oracle() {
        let mut state = 0x12345678u32;
        for size in (1..256).chain([256, 257, 4096, 4097, 65536, 65537]) {
            for domain in [2, 7, 256] {
                let input: Vec<_> = (0..size)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        (state % domain) as u8
                    })
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
                let (actual, origin) = super::transform(&input);
                assert_eq!(actual, expected);
                assert_eq!(
                    (0..size)
                        .map(|i| input[(rotations[origin] + i) % size])
                        .collect::<Vec<_>>(),
                    input
                );
            }
        }
    }
}
