//! Rotation sorting of primitive blocks by cyclic induced sorting.
//!
//! This is SA-IS (Nong, Zhang and Chan, "Two Efficient Algorithms for Linear
//! Time Suffix Array Construction", 2011) applied to the block's rotations
//! instead of its suffixes. Every position is typed by comparing rotation `i`
//! with rotation `i + 1` (modulo the block length), so there is no sentinel and
//! no doubled input: an S position precedes a larger rotation, an L position a
//! smaller one, and an LMS position is an S position preceded by an L one.
//!
//! Why the order is exact, for a primitive block `s` of length `n >= 2`:
//!
//! - The rotations of `s` are pairwise distinct, so every position has a type
//!   and the sorted rotation order is unique.
//! - `s` is not constant, so it has L and S positions and at least one LMS
//!   position. No two LMS positions are adjacent, so there are at most `n / 2`.
//! - The induction steps compare a rotation's first byte, then the rank of the
//!   rotation one position later. The usual SA-IS argument is local and holds
//!   unchanged on the infinite periodic string `s s s ...`.
//! - The reduced string of LMS-substring names is again primitive: were it a
//!   proper power, the LMS substrings it names would repeat the same way and
//!   `s` would be a proper power too. So the recursion meets the same
//!   precondition.
//!
//! Proper powers have tied rotations, and the existing sorter decides where
//! rotation 0 lands among its ties. [`Workspace::sort`] therefore routes those
//! blocks (and small ones) to [`flag_sort`](super::flag_sort), so the encoded
//! bytes never change.

use super::flag_sort;
use std::io;

// High bit of a suffix-array word: set when the word must not induce its
// predecessor in the current pass. Positions stay below 900,000.
const MARK: u32 = 1 << 31;
// An unoccupied slot. It carries MARK, so passes skip it.
const EMPTY: u32 = u32::MAX;
// Blocks shorter than this go to the existing sorter; see `sort`.
const MIN_INDUCED: usize = 2048;

trait Symbol: Copy + Ord {
    fn index(self) -> usize;
}

impl Symbol for u8 {
    #[inline(always)]
    fn index(self) -> usize {
        usize::from(self)
    }
}

impl Symbol for u32 {
    #[inline(always)]
    fn index(self) -> usize {
        self as usize
    }
}

#[inline(always)]
fn before(position: usize, len: usize) -> usize {
    if position == 0 { len - 1 } else { position - 1 }
}

/// Whether `input` is `u^k` for some `k >= 2`. If it is, then for a prime `q`
/// dividing `k` it is also `(u^(k/q))^q`, so it has the period `n / q` for a
/// prime `q` dividing `n`. Conversely such a period makes it a `q`-th power.
pub(super) fn is_proper_power(input: &[u8]) -> bool {
    let len = input.len();
    let periodic = |period: usize| input[period..] == input[..len - period];
    let mut rest = len;
    let mut prime = 2;
    while prime * prime <= rest {
        if rest.is_multiple_of(prime) {
            if periodic(len / prime) {
                return true;
            }
            while rest.is_multiple_of(prime) {
                rest /= prime;
            }
        }
        prime += 1;
    }
    rest > 1 && periodic(len / rest)
}

/// Sets bit `p` of `lms` for every LMS position `p` and returns their number.
/// `text` must be cyclically non-constant. Types are found right to left
/// without branches; the last position's type is that of the first position
/// at or after it, cyclically, whose successor differs from it.
fn mark_lms<T: Symbol>(text: &[T], lms: &mut [u32]) -> usize {
    let len = text.len();
    let mut i = len - 1;
    let last_is_s = loop {
        let next = if i + 1 == len { 0 } else { i + 1 };
        if text[i] != text[next] {
            break text[i] < text[next];
        }
        i = next;
    };
    let mut is_s = last_is_s;
    let mut total = 0;
    for (word, slot) in lms.iter_mut().enumerate().rev() {
        let low = word * 32;
        let high = (low + 32).min(len);
        let mut bits = 0;
        for p in (low.max(1)..high).rev() {
            let (previous, current) = (text[p - 1], text[p]);
            let previous_is_s = (previous < current) | ((previous == current) & is_s);
            bits |= u32::from(is_s & !previous_is_s) << (p - low);
            is_s = previous_is_s;
        }
        if low == 0 {
            bits |= u32::from(is_s & !last_is_s);
        }
        total += bits.count_ones() as usize;
        *slot = bits;
    }
    total
}

/// Calls `visit` with every LMS position in increasing order.
#[inline(always)]
fn for_each_lms(lms: &[u32], mut visit: impl FnMut(usize)) {
    for (word, &bits) in lms.iter().enumerate() {
        let mut bits = bits;
        while bits != 0 {
            visit(word * 32 + bits.trailing_zeros() as usize);
            bits &= bits - 1;
        }
    }
}

fn count<T: Symbol>(text: &[T], counts: &mut [u32]) {
    counts.fill(0);
    for &symbol in text {
        counts[symbol.index()] += 1;
    }
}

fn heads(counts: &[u32], pointers: &mut [u32]) {
    let mut total = 0;
    for (pointer, &count) in pointers.iter_mut().zip(counts) {
        *pointer = total;
        total += count;
    }
}

fn tails(counts: &[u32], pointers: &mut [u32]) {
    let mut total = 0;
    for (pointer, &count) in pointers.iter_mut().zip(counts) {
        total += count;
        *pointer = total;
    }
}

/// Sorts the LMS substrings, leaving the LMS positions in sorted
/// LMS-substring order in `sa[..m]`, where `m` is the number of LMS bits.
fn sort_lms_substrings<T: Symbol>(
    text: &[T],
    sa: &mut [u32],
    lms: &[u32],
    counts: &[u32],
    pointers: &mut [u32],
) {
    let len = text.len();
    sa.fill(EMPTY);
    tails(counts, pointers);
    for_each_lms(lms, |position| {
        let slot = &mut pointers[text[position].index()];
        *slot -= 1;
        sa[*slot as usize] = position as u32;
    });
    // L pass. A word whose predecessor is an L position induces it at its
    // bucket's head. Induced words whose own predecessor is S are marked;
    // they induce in the S pass instead. Spent words are cleared: only LMS
    // words have to survive this stage.
    heads(counts, pointers);
    for i in 0..len {
        let word = sa[i];
        if word & MARK == 0 {
            sa[i] = EMPTY;
            let position = before(word as usize, len);
            let symbol = text[position];
            let previous = text[before(position, len)];
            let slot = &mut pointers[symbol.index()];
            sa[*slot as usize] = position as u32 | if previous < symbol { MARK } else { 0 };
            *slot += 1;
        } else if word != EMPTY {
            sa[i] = word & !MARK;
        }
    }
    // S pass. Induced LMS words are marked and kept; all others are spent.
    tails(counts, pointers);
    for i in (0..len).rev() {
        let word = sa[i];
        if word & MARK == 0 {
            sa[i] = EMPTY;
            let position = before(word as usize, len);
            let symbol = text[position];
            let previous = text[before(position, len)];
            let slot = &mut pointers[symbol.index()];
            *slot -= 1;
            sa[*slot as usize] = position as u32 | if previous > symbol { MARK } else { 0 };
        }
    }
    // Keep the marked LMS words in order; the write index never passes the
    // read index, and a store of an unkept word is overwritten by the next.
    let mut kept = 0;
    for i in 0..len {
        let word = sa[i];
        sa[kept] = word & !MARK;
        kept += usize::from(word != EMPTY && word & MARK != 0);
    }
}

#[inline(always)]
fn same_substring<T: Symbol>(text: &[T], a: usize, b: usize, length: usize) -> bool {
    let len = text.len();
    if a.max(b) + length < len {
        // LMS substrings are short; an inline loop beats a memcmp call.
        let (a, b) = (&text[a..=a + length], &text[b..=b + length]);
        return a.iter().zip(b).all(|(a, b)| a == b);
    }
    (0..=length).all(|offset| {
        let (a, b) = (a + offset, b + offset);
        text[if a >= len { a - len } else { a }] == text[if b >= len { b - len } else { b }]
    })
}

/// Names the sorted LMS substrings in `sa[..m]` and writes the reduced string
/// (names in position order) to `sa[len - m..]`. Returns the number of
/// distinct names.
fn name_lms_substrings<T: Symbol>(text: &[T], sa: &mut [u32], m: usize, lms: &[u32]) -> usize {
    let len = text.len();
    // LMS positions are at least two apart, so `position / 2` is a private
    // slot in sa[m..]; m <= len / 2 keeps every slot below len. Each slot first
    // holds its LMS substring's length: the distance to the next LMS
    // position, cyclically.
    sa[m..].fill(0);
    let mut first = None;
    let mut previous = 0;
    for_each_lms(lms, |position| {
        if first.is_some() {
            sa[m + previous / 2] = (position - previous) as u32;
        } else {
            first = Some(position);
        }
        previous = position;
    });
    sa[m + previous / 2] = (first.unwrap() + len - previous) as u32;
    let mut names = 0;
    let mut last = (0, 0);
    for i in 0..m {
        let position = sa[i] as usize;
        let length = sa[m + position / 2] as usize;
        if i == 0 || last.1 != length || !same_substring(text, position, last.0, length) {
            names += 1;
        }
        sa[m + position / 2] = names as u32;
        last = (position, length);
    }
    // Gather the names in position order; empty slots hold zero. The write
    // index stays above the read index, and a store for an empty slot is
    // overwritten by the next name.
    let mut write = len;
    for read in (m..len).rev() {
        let name = sa[read];
        sa[write - 1] = name.wrapping_sub(1);
        write -= usize::from(name != 0);
    }
    debug_assert_eq!(write, len - m);
    names
}

/// Sorts the rotations of the reduced string held in `sa[len - m..]` into
/// `sa[..m]`, as indices into that string.
fn sort_reduced(sa: &mut [u32], m: usize, names: usize, lms: &mut [u32], spare: &mut [u32]) {
    let len = sa.len();
    let (order, rest) = sa.split_at_mut(m);
    let reduced = &rest[len - 2 * m..];
    if names == m {
        for (index, &name) in reduced.iter().enumerate() {
            order[name as usize] = index as u32;
        }
    } else {
        sort_names(reduced, order, names, lms, spare);
    }
}

/// Places the sorted LMS positions and induces every other position.
fn induce<T: Symbol>(
    text: &[T],
    sa: &mut [u32],
    m: usize,
    lms: &[u32],
    counts: &[u32],
    pointers: &mut [u32],
) {
    let len = text.len();
    // Replace the reduced string with the LMS positions in position order,
    // then map reduced indices to positions.
    let mut write = len - m;
    for_each_lms(lms, |position| {
        sa[write] = position as u32;
        write += 1;
    });
    for i in 0..m {
        sa[i] = sa[len - m + sa[i] as usize];
    }
    sa[m..].fill(EMPTY);
    tails(counts, pointers);
    // The i-th smallest LMS position lands at or after slot i, so moving them
    // from the highest down never overwrites one not yet moved.
    for i in (0..m).rev() {
        let position = sa[i];
        sa[i] = EMPTY;
        let slot = &mut pointers[text[position as usize].index()];
        *slot -= 1;
        sa[*slot as usize] = position;
    }
    // L pass: as in `sort_lms_substrings`, but spent words stay, marked so
    // the S pass skips them.
    heads(counts, pointers);
    for i in 0..len {
        let word = sa[i];
        if word & MARK == 0 {
            sa[i] = word | MARK;
            let position = before(word as usize, len);
            let symbol = text[position];
            let previous = text[before(position, len)];
            let slot = &mut pointers[symbol.index()];
            sa[*slot as usize] = position as u32 | if previous < symbol { MARK } else { 0 };
            *slot += 1;
        } else if word != EMPTY {
            sa[i] = word & !MARK;
        }
    }
    // S pass: rewrites every S slot and clears every mark.
    tails(counts, pointers);
    for i in (0..len).rev() {
        let word = sa[i];
        if word & MARK == 0 {
            let position = before(word as usize, len);
            let symbol = text[position];
            let previous = text[before(position, len)];
            let slot = &mut pointers[symbol.index()];
            *slot -= 1;
            sa[*slot as usize] = position as u32 | if previous > symbol { MARK } else { 0 };
        } else {
            debug_assert_ne!(word, EMPTY);
            sa[i] = word & !MARK;
        }
    }
}

/// Sorts the rotations of a primitive byte block of length >= 2. `lms` holds
/// at least `n / 16 + 32` words and `spare` at least `n`.
fn sort_bytes(text: &[u8], sa: &mut [u32], lms: &mut [u32], spare: &mut [u32]) {
    let mut counts = [0; 256];
    let mut pointers = [0; 256];
    count(text, &mut counts);
    let (lms, deeper) = lms.split_at_mut(text.len().div_ceil(32));
    let m = mark_lms(text, lms);
    sort_lms_substrings(text, sa, lms, &counts, &mut pointers);
    let names = name_lms_substrings(text, sa, m, lms);
    sort_reduced(sa, m, names, deeper, spare);
    induce(text, sa, m, lms, &counts, &mut pointers);
}

/// Sorts the rotations of a primitive reduced string of length >= 2 whose
/// symbols are below `names`. Its buckets take `2 * names` words of `spare`;
/// deeper levels reuse that space, so they are recounted after recursing.
fn sort_names(text: &[u32], sa: &mut [u32], names: usize, lms: &mut [u32], spare: &mut [u32]) {
    let (lms, deeper) = lms.split_at_mut(text.len().div_ceil(32));
    let m = mark_lms(text, lms);
    let (counts, pointers) = spare[..2 * names].split_at_mut(names);
    count(text, counts);
    sort_lms_substrings(text, sa, lms, counts, pointers);
    let deeper_names = name_lms_substrings(text, sa, m, lms);
    sort_reduced(sa, m, deeper_names, deeper, spare);
    let (counts, pointers) = spare[..2 * names].split_at_mut(names);
    if deeper_names < m {
        count(text, counts);
    }
    induce(text, sa, m, lms, counts, pointers);
}

/// Rotation sorter for encoder blocks. Primitive blocks of at least
/// `MIN_INDUCED` bytes are induced-sorted; all others use the existing sorter.
///
/// Memory: the induced path uses `2n + n / 16 + 32` words of one buffer shared
/// with the existing sorter, which needs `n + 65,536 + C` words, where `C` is
/// its rank capacity (at least `n`). The buffer holds the larger of the two,
/// never their sum. At `n = 900,000` that is 7.46 MB on macOS ARM64 and
/// 8.06 MB elsewhere, as before this sorter existed.
#[derive(Default)]
pub(super) struct Workspace {
    fallback: flag_sort::Workspace,
}

impl Workspace {
    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.fallback.capacity()
    }

    pub(super) fn sort(&mut self, input: &[u8]) -> io::Result<(&[u32], usize)> {
        assert!(!input.is_empty() && input.len() <= 900_000);
        if input.len() < MIN_INDUCED || is_proper_power(input) {
            return self.fallback.sort(input);
        }
        self.induced(input)
    }

    /// The induced path alone. `input` must be primitive with length >= 2.
    pub(super) fn induced(&mut self, input: &[u8]) -> io::Result<(&[u32], usize)> {
        assert!(input.len() >= 2 && input.len() <= 900_000);
        let len = input.len();
        let storage = self.fallback.storage();
        // Suffix array, bucket spare for the reduced levels, then one LMS
        // bitmap per level: level sizes at least halve, so their bitmaps
        // take at most 2n / 32 words plus one partial word per level.
        let needed = 2 * len + len / 16 + 32;
        if storage.len() < needed {
            storage
                .try_reserve_exact(needed - storage.len())
                .map_err(io::Error::other)?;
            storage.resize(needed, 0);
        }
        let (sa, rest) = storage[..needed].split_at_mut(len);
        let (spare, lms) = rest.split_at_mut(len);
        sort_bytes(input, sa, lms, spare);
        let origin = sa.iter().position(|&position| position == 0).unwrap();
        Ok((sa, origin))
    }
}

#[cfg(test)]
mod tests;
