//! Reusable EncodeBlock MTF/Huffman storage.
// Translation of BZip2Encoder.cpp from 7-Zip, LGPL-2.1-or-later.
// https://github.com/ip7z/7zip/blob/9128b80e3a471108678e2b3ec3c985861c8d7b0f/CPP/7zip/Compress/BZip2Encoder.cpp
// Changes: typed initialized buffers and fixed Huffman scratch arrays.

use super::trial::TrialWriter;
use std::io;

const ALPHABET: usize = 258;
const TABLES: usize = 6;
const SELECTORS: usize = 900_001usize.div_ceil(50);

struct Huffman {
    parents: [u16; 2 * ALPHABET - 1],
    weights: [u32; 2 * ALPHABET - 1],
    leaves: [(u32, u16); ALPHABET],
}

impl Huffman {
    fn new() -> Self {
        Self {
            parents: [u16::MAX; 2 * ALPHABET - 1],
            weights: [0; 2 * ALPHABET - 1],
            leaves: [(0, 0); ALPHABET],
        }
    }

    // Huffman_Generate's two monotone queues and length-limited population.
    // Same tie ordering as the existing checked port; storage is reused.
    fn generate(&mut self, frequencies: &[u32], lengths: &mut [u8]) {
        let n = frequencies.len();
        assert!((2..=ALPHABET).contains(&n));
        let nodes = 2 * n - 1;
        self.parents[..nodes].fill(u16::MAX);
        self.weights[..nodes].fill(0);
        for (i, &frequency) in frequencies.iter().enumerate() {
            self.leaves[i] = (frequency.max(1), i as u16);
        }
        self.leaves[..n].sort_unstable();
        let mut leaf = 0;
        let mut internal = n;
        for node in n..nodes {
            for _ in 0..2 {
                let (weight, child) = if internal == node
                    || (leaf < n && self.leaves[leaf].0 <= self.weights[internal])
                {
                    let (weight, child) = self.leaves[leaf];
                    leaf += 1;
                    (weight, usize::from(child))
                } else {
                    let result = (self.weights[internal], internal);
                    internal += 1;
                    result
                };
                self.parents[child] = node as u16;
                self.weights[node] += weight;
            }
        }
        let mut population = [0usize; 17];
        population[1] = 2;
        for i in (n..nodes - 1).rev() {
            let mut depth = 0;
            let mut node = i;
            while self.parents[node] != u16::MAX {
                node = usize::from(self.parents[node]);
                depth += 1;
            }
            if depth >= 16 {
                depth = (1..16).rev().find(|&i| population[i] != 0).unwrap();
            }
            population[depth] -= 1;
            population[depth + 1] += 2;
        }
        let mut leaf = 0;
        for depth in (1..=16).rev() {
            for _ in 0..population[depth] {
                lengths[usize::from(self.leaves[leaf].1)] = depth as u8;
                leaf += 1;
            }
        }
    }
}

// Byte-symbol representation from EncodeBlock: 0xff introduces a second byte
// and represents symbol (255 + second). Length counts logical symbols, not
// escaped storage bytes; selector boundaries are always 50 logical symbols.
#[derive(Clone)]
struct Symbols<'a> {
    bytes: std::slice::Iter<'a, u8>,
}

impl Iterator for Symbols<'_> {
    type Item = u16;
    fn next(&mut self) -> Option<Self::Item> {
        let first = u16::from(*self.bytes.next()?);
        Some(if first == 255 {
            first + u16::from(*self.bytes.next().expect("complete internal MTF escape"))
        } else {
            first
        })
    }
}

pub(super) struct Entropy {
    mtf: Vec<u8>,
    count: usize,
    alphabet: usize,
    counts: [u32; ALPHABET],
    lens: [[u8; ALPHABET]; TABLES],
    frequencies: [[u32; ALPHABET]; TABLES],
    codes: [[u32; ALPHABET]; TABLES],
    selectors: [u8; SELECTORS],
    selector_count: usize,
    huffman: Huffman,
}

impl Entropy {
    pub(super) fn new() -> Self {
        Self {
            mtf: Vec::new(),
            count: 0,
            alphabet: 0,
            counts: [0; ALPHABET],
            lens: [[0; ALPHABET]; TABLES],
            frequencies: [[0; ALPHABET]; TABLES],
            codes: [[0; ALPHABET]; TABLES],
            selectors: [0; SELECTORS],
            selector_count: 0,
            huffman: Huffman::new(),
        }
    }

    fn symbol(&mut self, symbol: u16) {
        assert!(usize::from(symbol) < self.alphabet);
        if symbol >= 255 {
            self.mtf.push(255);
            self.mtf.push((symbol - 255) as u8);
        } else {
            self.mtf.push(symbol as u8);
        }
        self.counts[usize::from(symbol)] += 1;
        self.count += 1;
    }

    fn zeros(&mut self, mut count: usize) {
        while count != 0 {
            count -= 1;
            self.symbol((count & 1) as u16);
            count >>= 1;
        }
    }

    pub(super) fn prepare(&mut self, input: &[u8], order: &[u32]) -> io::Result<()> {
        assert!(!input.is_empty() && input.len() <= 900_000);
        assert_eq!(input.len(), order.len());
        self.mtf.clear();
        self.count = 0;
        self.counts.fill(0);
        let mut present = [false; 256];
        for &byte in input {
            present[usize::from(byte)] = true;
        }
        let mut alphabet = [0u64; 32];
        let mut used = 0;
        for (byte, &exists) in present.iter().enumerate() {
            if exists {
                alphabet[used / 8] |= (byte as u64) << ((used & 7) * 8);
                used += 1;
            }
        }
        self.alphabet = used + 2;
        // Each nonzero MTF symbol takes at most two bytes. Reserve only once
        // per largest input; initialization still follows actual encoded size.
        let needed = 2 * (input.len() + 1);
        if self.mtf.capacity() < needed {
            self.mtf
                .try_reserve_exact(needed)
                .map_err(io::Error::other)?;
        }
        let mut zeros = 0;
        for &position in order {
            let position = position as usize;
            // Workspace::sort returned marker-free, validated original indices.
            let byte = input[if position == 0 {
                input.len() - 1
            } else {
                position - 1
            }];
            if alphabet[0] as u8 == byte {
                zeros += 1;
            } else {
                self.zeros(zeros);
                zeros = 0;
                self.symbol((super::find_and_move(&mut alphabet, byte) + 1) as u16);
            }
        }
        self.zeros(zeros);
        self.symbol((used + 1) as u16);
        assert!(self.count <= input.len() + 1);
        Ok(())
    }

    pub(super) fn symbol_count(&self) -> usize {
        self.count
    }

    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.mtf.capacity()
    }

    fn symbols(&self) -> Symbols<'_> {
        Symbols {
            bytes: self.mtf.iter(),
        }
    }

    fn fit(&mut self, groups: usize) {
        assert!((2..=TABLES).contains(&groups));
        let alphabet = self.alphabet;
        for lens in &mut self.lens[..groups] {
            lens[..alphabet].fill(1);
        }
        let mut remaining = self.count;
        let mut start = 0;
        for partition in 0..groups {
            let target = remaining / (groups - partition);
            let mut end = start;
            let mut sum = 0;
            while end < alphabet && sum < target {
                sum += self.counts[end] as usize;
                end += 1;
            }
            if end > start + 1 && partition != 0 && partition + 1 != groups && partition & 1 != 0 {
                end -= 1;
                sum -= self.counts[end] as usize;
            }
            self.lens[groups - partition - 1][start..end].fill(0);
            remaining -= sum;
            start = end;
        }
        self.selector_count = self.count.div_ceil(50);
        for _ in 0..4 {
            for frequency in &mut self.frequencies[..groups] {
                frequency[..alphabet].fill(0);
            }
            let mut symbols = Symbols {
                bytes: self.mtf.iter(),
            };
            for group in 0..self.selector_count {
                let mut chunk = [0u16; 50];
                let mut size = 0;
                for symbol in symbols.by_ref().take(50) {
                    chunk[size] = symbol;
                    size += 1;
                }
                let mut best_price = u32::MAX;
                let mut best = 0;
                for table in 0..groups {
                    let price: u32 = chunk[..size]
                        .iter()
                        .map(|&symbol| u32::from(self.lens[table][usize::from(symbol)]))
                        .sum();
                    if price < best_price {
                        best_price = price;
                        best = table;
                    }
                }
                self.selectors[group] = best as u8;
                for &symbol in &chunk[..size] {
                    self.frequencies[best][usize::from(symbol)] += 1;
                }
            }
            for table in 0..groups {
                self.huffman.generate(
                    &self.frequencies[table][..alphabet],
                    &mut self.lens[table][..alphabet],
                );
            }
        }
        for table in 0..groups {
            let mut code = 0;
            for length in 1..=16 {
                for symbol in 0..alphabet {
                    if self.lens[table][symbol] == length {
                        self.codes[table][symbol] = code;
                        code += 1;
                    }
                }
                code <<= 1;
            }
        }
    }

    pub(super) fn encode(&mut self, groups: usize, writer: &mut TrialWriter) -> io::Result<()> {
        self.fit(groups);
        writer.put(groups as u32, 3)?;
        writer.put(self.selector_count as u32, 15)?;
        let mut order = [0u8, 1, 2, 3, 4, 5];
        for &selector in &self.selectors[..self.selector_count] {
            let index = order[..groups]
                .iter()
                .position(|&table| table == selector)
                .unwrap();
            for _ in 0..index {
                writer.put(1, 1)?;
            }
            writer.put(0, 1)?;
            order[..=index].rotate_right(1);
        }
        for table in &self.lens[..groups] {
            let mut previous = table[0];
            writer.put(u32::from(previous), 5)?;
            for &length in &table[..self.alphabet] {
                while previous < length {
                    writer.put(2, 2)?;
                    previous += 1;
                }
                while previous > length {
                    writer.put(3, 2)?;
                    previous -= 1;
                }
                writer.put(0, 1)?;
            }
        }
        for (i, symbol) in self.symbols().enumerate() {
            let table = usize::from(self.selectors[i / 50]);
            let symbol = usize::from(symbol);
            writer.put(self.codes[table][symbol], self.lens[table][symbol])?;
        }
        Ok(())
    }

    // EncodeBlock's table search rewinds the same cursor for each candidate,
    // then re-encodes the best table count if the final candidate did not win.
    pub(super) fn optimize(&mut self, writer: &mut TrialWriter) -> io::Result<usize> {
        let start = writer.checkpoint();
        let initial = writer.bit_len();
        let mut best = 2;
        let mut price = usize::MAX;
        for groups in 2..=6 {
            writer.restore(start);
            self.encode(groups, writer)?;
            let candidate = writer.bit_len() - initial;
            if candidate <= price {
                price = candidate;
                best = groups;
            }
        }
        if best != 6 {
            writer.restore(start);
            self.encode(best, writer)?;
        }
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        Bits, codes, encode_symbols, marked_sort::Workspace, symbols, tables, trial::TrialWriter,
    };
    use super::{ALPHABET, Entropy, Huffman};

    fn full_bytes(bits: &Bits, alignment: u8) -> (Vec<u8>, usize) {
        let mut vector: Vec<bool> = (0..alignment).map(|i| i % 2 == 0).collect();
        for &byte in &bits.bytes {
            vector.extend((0..8).rev().map(|i| byte >> i & 1 != 0));
        }
        vector.extend((0..bits.count).rev().map(|i| bits.pending >> i & 1 != 0));
        let bytes = vector
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0, |v, (i, &b)| v | (u8::from(b) << (7 - i)))
            })
            .collect();
        (bytes, vector.len())
    }

    fn prefix(writer: &mut TrialWriter, alignment: u8) {
        for i in 0..alignment {
            writer.put(u32::from(i % 2 == 0), 1).unwrap();
        }
    }

    #[test]
    fn direct_order_mtf_and_fixed_tables_match_every_old_intermediate_and_bits() {
        let mut entropy = Entropy::new();
        let mut workspace = Workspace::default();
        let mut state = 0x1923_ab5du32;
        for domain in [1, 2, 7, 254, 255, 256] {
            for size in [
                1, 2, 49, 50, 51, 199, 200, 599, 600, 1199, 1200, 2399, 2400, 4097,
            ] {
                let input: Vec<_> = (0..size)
                    .map(|i| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        if size >= 256 && i < domain {
                            i as u8
                        } else {
                            (state as usize % domain) as u8
                        }
                    })
                    .collect();
                let (order, _) = workspace.sort(&input).unwrap();
                let transformed: Vec<_> = order
                    .iter()
                    .map(|&i| input[if i == 0 { size - 1 } else { i as usize - 1 }])
                    .collect();
                let mut used = input.clone();
                used.sort_unstable();
                used.dedup();
                let old_symbols = symbols(&transformed, &used);
                entropy.prepare(&input, order).unwrap();
                assert_eq!(entropy.symbols().collect::<Vec<_>>(), old_symbols);
                assert_eq!(entropy.count, old_symbols.len());
                let mut frequencies = [0u32; ALPHABET];
                for &symbol in &old_symbols {
                    frequencies[usize::from(symbol)] += 1;
                }
                assert_eq!(entropy.counts, frequencies);
                for groups in 2..=6 {
                    entropy.fit(groups);
                    let (old_lens, old_selectors) = tables(&old_symbols, used.len() + 2, groups);
                    assert_eq!(
                        &entropy.selectors[..entropy.selector_count],
                        old_selectors.iter().map(|&i| i as u8).collect::<Vec<_>>()
                    );
                    for (table, old_lengths) in old_lens.iter().enumerate() {
                        assert_eq!(&entropy.lens[table][..entropy.alphabet], old_lengths);
                        assert_eq!(
                            &entropy.codes[table][..entropy.alphabet],
                            codes(old_lengths)
                        );
                        let mut counts = vec![0u32; entropy.alphabet];
                        for (chunk, &selected) in old_symbols.chunks(50).zip(&old_selectors) {
                            if selected == table {
                                for &symbol in chunk {
                                    counts[usize::from(symbol)] += 1;
                                }
                            }
                        }
                        assert_eq!(&entropy.frequencies[table][..entropy.alphabet], counts);
                    }
                    let expected = encode_symbols(&old_symbols, entropy.alphabet, groups);
                    for alignment in 0..8 {
                        let mut writer = TrialWriter::new();
                        prefix(&mut writer, alignment);
                        entropy.encode(groups, &mut writer).unwrap();
                        let (bytes, len) = full_bytes(&expected, alignment);
                        assert_eq!(writer.output(), bytes);
                        assert_eq!(writer.bit_len(), len);
                    }
                }
            }
        }
    }

    #[test]
    fn optimized_group_trials_reuse_cursor_and_preserve_later_ties() {
        let mut entropy = Entropy::new();
        let mut workspace = Workspace::default();
        for size in [1i32, 2, 7, 50, 199, 600, 2400, 65537] {
            let input: Vec<_> = (0..size)
                // Preserve the release fixture's signed wrapping polynomial
                // before taking its remainder, in debug builds as well.
                .map(|i| {
                    (i.wrapping_mul(i)
                        .wrapping_add(i.wrapping_mul(17))
                        .wrapping_add(size)
                        % 251) as u8
                })
                .collect();
            let (order, _) = workspace.sort(&input).unwrap();
            entropy.prepare(&input, order).unwrap();
            let logical: Vec<_> = entropy.symbols().collect();
            let candidates: Vec<_> = (2..=6)
                .map(|g| encode_symbols(&logical, entropy.alphabet, g))
                .collect();
            let best = (0..5).rev().min_by_key(|&i| candidates[i].len()).unwrap();
            for alignment in 0..8 {
                let mut writer = TrialWriter::new();
                prefix(&mut writer, alignment);
                assert_eq!(entropy.optimize(&mut writer).unwrap(), best + 2);
                let (bytes, len) = full_bytes(&candidates[best], alignment);
                assert_eq!(writer.output(), bytes);
                assert_eq!(writer.bit_len(), len);
            }
        }
    }

    #[test]
    fn fixed_huffman_scratch_preserves_ties_limits_and_large_to_small_reuse() {
        let mut huffman = Huffman::new();
        let mut result = [0u8; ALPHABET];
        for size in [258, 2, 3, 17, 258, 7] {
            for kind in 0..3 {
                let mut frequencies = vec![1u32; size];
                if kind == 1 {
                    frequencies[0] = 900000;
                }
                if kind == 2 {
                    for i in 2..size.min(29) {
                        frequencies[i] = frequencies[i - 1] + frequencies[i - 2];
                    }
                }
                huffman.generate(&frequencies, &mut result[..size]);
                assert_eq!(&result[..size], super::super::lengths(&frequencies));
            }
        }
    }

    #[test]
    fn full_block_then_small_alphabets_keep_capacity_without_stale_symbols() {
        let mut workspace = Workspace::default();
        let mut entropy = Entropy::new();
        let input: Vec<_> = (0..900_000usize)
            .map(|i| ((i * 73 + i / 251) & 255) as u8)
            .collect();
        let (order, _) = workspace.sort(&input).unwrap();
        entropy.prepare(&input, order).unwrap();
        let decoded: Vec<_> = entropy.symbols().collect();
        let transformed: Vec<_> = order
            .iter()
            .map(|&position| {
                input[if position == 0 {
                    input.len() - 1
                } else {
                    position as usize - 1
                }]
            })
            .collect();
        assert_eq!(
            decoded,
            symbols(&transformed, &(0..=255).collect::<Vec<_>>())
        );
        assert!(entropy.mtf.len() <= 2 * (input.len() + 1));
        let capacity = entropy.mtf.capacity();
        for input in [
            b"x".as_slice(),
            b"ababababababab",
            b"short alphabet",
            &[0; 257],
        ] {
            let (order, _) = workspace.sort(input).unwrap();
            entropy.prepare(input, order).unwrap();
            let mut fresh = Entropy::new();
            fresh.prepare(input, order).unwrap();
            assert_eq!(entropy.mtf, fresh.mtf);
            assert_eq!(entropy.counts, fresh.counts);
            assert_eq!(entropy.count, fresh.count);
            assert_eq!(entropy.mtf.capacity(), capacity);
            for groups in [6, 2] {
                let mut expected = TrialWriter::new();
                let mut actual = TrialWriter::new();
                fresh.encode(groups, &mut expected).unwrap();
                entropy.encode(groups, &mut actual).unwrap();
                assert_eq!(actual.output(), expected.output());
                assert_eq!(actual.bit_len(), expected.bit_len());
            }
        }
    }

    #[test]
    fn escapes_cross_logical_selector_edges_and_fill_the_final_selector() {
        for count in [101, 900_001] {
            let logical: Vec<u16> = (0..count)
                .map(|i| match i {
                    49 | 100 => 255,
                    50 => 256,
                    99 => 257,
                    _ => ((i * 17) % 258) as u16,
                })
                .collect();
            let mut entropy = Entropy::new();
            entropy.alphabet = ALPHABET;
            entropy.mtf.reserve_exact(2 * count);
            for &symbol in &logical {
                entropy.symbol(symbol);
            }
            assert_eq!(entropy.symbols().collect::<Vec<_>>(), logical);
            entropy.fit(6);
            assert_eq!(entropy.selector_count, count.div_ceil(50));
            if count == 900_001 {
                assert_eq!(entropy.selector_count, super::SELECTORS);
            }
            let (lengths, selectors) = tables(&logical, ALPHABET, 6);
            assert_eq!(
                &entropy.selectors[..entropy.selector_count],
                selectors.iter().map(|&s| s as u8).collect::<Vec<_>>()
            );
            for (table, lengths) in lengths.iter().enumerate() {
                assert_eq!(entropy.lens[table], lengths.as_slice());
                assert_eq!(entropy.codes[table], codes(lengths).as_slice());
            }
            let expected = encode_symbols(&logical, ALPHABET, 6);
            // All alignments are covered above on short fixtures; this also
            // checks the complete maximum-selector stream at a partial byte.
            let mut writer = TrialWriter::new();
            prefix(&mut writer, 7);
            entropy.encode(6, &mut writer).unwrap();
            let (bytes, len) = full_bytes(&expected, 7);
            assert_eq!(writer.output(), bytes);
            assert_eq!(writer.bit_len(), len);
        }
    }
}
