//! Reusable bit-cursor storage for the encoder's recursive trials.
// Translation of EncodeBlock2's cursor handling from 7-Zip, LGPL-2.1-or-later.
// https://github.com/ip7z/7zip/blob/9128b80e3a471108678e2b3ec3c985861c8d7b0f/CPP/7zip/Compress/BZip2Encoder.cpp
// Changes: checked initialized storage, owned cursors and bounded CRC metadata.

use std::io;

// At most n+1 MTF symbols, 16 bits per Huffman symbol, six selector bits per
// 50-symbol group, six tables of (5 + 31*258) length-description bits, and
// 377+18 block/table-count header bits give a one-block byte bound B(n).
// Including rounding and a carry byte, B(n) <= 17*n/8 + 6060.
//
// At a recursive split a+b=n, the preserved left winner is <= B(a). The
// active right subtree needs P(b,p-1); the final whole trial coexists with
// both selected children, requiring B(a)+B(b)+B(n). Their alignment/carry
// allowances are respectively +1 and +2 bytes. Induction therefore gives
// P(n,p) <= ceil(17*n/4) + (p+2)*6060 + 2*p, for p<=10. This conservative
// bound does not assume balanced splits. At n=900000,p=10 it is 3,897,740
// bytes, below 4 MiB. It is not a resident-memory claim: initialized prefixes
// grow in 4 KiB chunks (up to 4,095 bytes ahead), clamped to this ceiling;
// no bound-sized allocation is made up front.
const MAX_TRIAL_BYTES: usize = (17usize * 900_000).div_ceil(4) + 12 * 6060 + 20;
const _: () = assert!(MAX_TRIAL_BYTES < 4 * 1024 * 1024);
const MAX_CRCS: usize = 1 << 10;

#[derive(Clone, Copy, Debug)]
pub(super) struct Cursor {
    bits: usize,
    partial: u8,
    crcs: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Choice {
    Divided,
    Whole,
}

pub(super) struct TrialWriter {
    // Length is the initialized high-water range, not the current output size.
    // Restoring a cursor must not discard a preserved alternative's bytes.
    bytes: Vec<u8>,
    bits: usize,
    pending: u8,
    crcs: [u32; MAX_CRCS],
    crc_count: usize,
    limit: usize,
}

impl TrialWriter {
    pub(super) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bits: 0,
            pending: 0,
            crcs: [0; MAX_CRCS],
            crc_count: 0,
            limit: MAX_TRIAL_BYTES,
        }
    }

    #[inline]
    fn initialize(&mut self, end: usize) -> io::Result<()> {
        // Production's limit is fixed; initialized length never exceeds it.
        if end <= self.bytes.len() {
            return Ok(());
        }
        self.refill(end)
    }

    // Initialization is amortized over one page, rather than zeroing the
    // output byte immediately before every Huffman symbol overwrites it.
    // This touches at most 4,095 bytes beyond the actual trial high-water mark.
    #[cold]
    #[inline(never)]
    fn refill(&mut self, end: usize) -> io::Result<()> {
        if end > self.limit {
            return Err(io::Error::other("BZip2 trial storage limit exceeded"));
        }
        let initialized = end
            .saturating_add(4095)
            .min(self.limit.saturating_add(4095))
            & !4095;
        let initialized = initialized.min(self.limit);
        if initialized > self.bytes.capacity() {
            let capacity = initialized
                .max(self.bytes.capacity().saturating_mul(2))
                .max(64)
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(io::Error::other)?;
        }
        if initialized > self.bytes.len() {
            self.bytes.resize(initialized, 0);
        }
        Ok(())
    }

    pub(super) fn checkpoint(&self) -> Cursor {
        let offset = self.bits & 7;
        Cursor {
            bits: self.bits,
            partial: if offset == 0 {
                0
            } else {
                self.pending << (8 - offset)
            },
            crcs: self.crc_count,
        }
    }

    pub(super) fn restore(&mut self, cursor: Cursor) {
        assert!(cursor.bits.div_ceil(8) <= self.bytes.len());
        assert!(cursor.crcs <= MAX_CRCS);
        if cursor.bits & 7 != 0 {
            self.bytes[cursor.bits / 8] = cursor.partial;
        }
        self.bits = cursor.bits;
        self.pending = if cursor.bits & 7 == 0 {
            0
        } else {
            cursor.partial >> (8 - (cursor.bits & 7))
        };
        self.crc_count = cursor.crcs;
    }

    #[inline(always)]
    pub(super) fn put(&mut self, value: u32, width: u8) -> io::Result<()> {
        if width > 32 || u64::from(value) >> width != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "BZip2 bit value exceeds its width",
            ));
        }
        let end = self
            .bits
            .checked_add(usize::from(width))
            .ok_or_else(|| io::Error::other("BZip2 trial bit position overflow"))?;
        self.initialize(end.div_ceil(8))?;
        if width == 0 {
            return Ok(());
        }
        let mut byte = self.bits / 8;
        let offset = self.bits & 7;
        let combined = (u64::from(self.pending) << width) | u64::from(value);
        let mut remaining = offset + usize::from(width);
        while remaining >= 8 {
            remaining -= 8;
            self.bytes[byte] = (combined >> remaining) as u8;
            byte += 1;
        }
        if remaining != 0 {
            self.bytes[byte] = (combined << (8 - remaining)) as u8;
        }
        self.pending = (combined & ((1u64 << remaining) - 1)) as u8;
        self.bits = end;
        Ok(())
    }

    pub(super) fn commit_crc(&mut self, crc: u32) -> io::Result<()> {
        let slot = self
            .crcs
            .get_mut(self.crc_count)
            .ok_or_else(|| io::Error::other("BZip2 trial CRC limit exceeded"))?;
        *slot = crc;
        self.crc_count += 1;
        Ok(())
    }

    // EncodeBlock2: keep divided bytes intact, then begin the whole trial at
    // the same bit alignment after the divided trial's final partial byte.
    pub(super) fn begin_whole(&mut self, start: Cursor, divided: Cursor) -> io::Result<Cursor> {
        assert!(start.bits <= divided.bits);
        assert_eq!(self.bits, divided.bits);
        let bits = divided
            .bits
            .div_ceil(8)
            .checked_mul(8)
            .and_then(|bits| bits.checked_add(start.bits & 7))
            .ok_or_else(|| io::Error::other("BZip2 trial alignment overflow"))?;
        self.initialize(bits.div_ceil(8))?;
        if bits & 7 != 0 {
            self.bytes[bits / 8] = start.partial;
        }
        self.bits = bits;
        self.pending = if bits & 7 == 0 {
            0
        } else {
            start.partial >> (8 - (bits & 7))
        };
        Ok(self.checkpoint())
    }

    // The whole CRC stays local until selection. In particular, it is never
    // appended behind all 512 possible divided leaf CRCs before rollback.
    pub(super) fn select(
        &mut self,
        start: Cursor,
        divided: Cursor,
        whole_start: Cursor,
        whole_crc: u32,
    ) -> io::Result<Choice> {
        assert!(start.bits <= divided.bits);
        assert!(whole_start.bits <= self.bits);
        assert_eq!(start.bits & 7, whole_start.bits & 7);
        let whole_len = self.bits - whole_start.bits;
        if whole_len >= divided.bits - start.bits {
            self.restore(divided);
            return Ok(Choice::Divided);
        }
        if start.crcs >= MAX_CRCS {
            return Err(io::Error::other("BZip2 trial CRC limit exceeded"));
        }
        let end = self.bits.div_ceil(8);
        self.bytes
            .copy_within(whole_start.bits / 8..end, start.bits / 8);
        self.bits = start.bits + whole_len;
        if self.bits & 7 != 0 {
            self.bytes[self.bits / 8] &= u8::MAX << (8 - (self.bits & 7));
        }
        self.crc_count = start.crcs;
        self.commit_crc(whole_crc)?;
        Ok(Choice::Whole)
    }

    /// Appends another writer's output and CRCs, exactly as if they had been
    /// written here.
    pub(super) fn append(&mut self, other: &TrialWriter) -> io::Result<()> {
        let output = other.output();
        for word in output[..other.bits / 8].chunks(4) {
            let value = word
                .iter()
                .fold(0u32, |value, &byte| (value << 8) | u32::from(byte));
            self.put(value, word.len() as u8 * 8)?;
        }
        let partial = other.bits & 7;
        if partial != 0 {
            self.put(
                u32::from(output[other.bits / 8] >> (8 - partial)),
                partial as u8,
            )?;
        }
        for &crc in other.crcs() {
            self.commit_crc(crc)?;
        }
        Ok(())
    }

    pub(super) fn bit_len(&self) -> usize {
        self.bits
    }

    pub(super) fn reset(&mut self) {
        self.bits = 0;
        self.pending = 0;
        self.crc_count = 0;
    }

    pub(super) fn crcs(&self) -> &[u32] {
        &self.crcs[..self.crc_count]
    }

    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    pub(super) fn output(&self) -> &[u8] {
        &self.bytes[..self.bits.div_ceil(8)]
    }
}

#[cfg(test)]
mod tests {
    use super::{Choice, MAX_CRCS, TrialWriter};

    fn bits(value: u32, width: u8) -> Vec<bool> {
        (0..width).rev().map(|bit| value >> bit & 1 != 0).collect()
    }

    fn packed(input: &[bool]) -> Vec<u8> {
        input
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0, |byte, (i, &bit)| byte | (u8::from(bit) << (7 - i)))
            })
            .collect()
    }

    fn write(writer: &mut TrialWriter, input: &[bool]) {
        for chunk in input.chunks(29) {
            let value = chunk.iter().fold(0, |v, &bit| (v << 1) | u32::from(bit));
            writer.put(value, chunk.len() as u8).unwrap();
        }
    }

    #[test]
    fn reusable_words_and_saved_partial_bytes_match_a_bit_vector() {
        for alignment in 0..8 {
            let mut writer = TrialWriter::new();
            let mut expected = bits(0x96, 8);
            expected.extend(bits(0x55 & ((1 << alignment) - 1), alignment));
            write(&mut writer, &expected);
            let saved = writer.checkpoint();
            let mut state = 0x1923_ab5du32;
            for width in (0..=32).cycle().take(330) {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let value = (u64::from(state) & ((1u64 << width) - 1)) as u32;
                writer.put(value, width).unwrap();
                expected.extend(bits(value, width));
                assert_eq!(writer.output(), packed(&expected));
                assert_eq!(writer.bits, expected.len());
            }
            let capacity = writer.bytes.capacity();
            writer.restore(saved);
            expected.truncate(saved.bits);
            write(&mut writer, &[false, true, true, false, false]);
            expected.extend([false, true, true, false, false]);
            assert_eq!(writer.output(), packed(&expected));
            assert_eq!(writer.bytes.capacity(), capacity);
        }
    }

    #[test]
    fn every_trial_alignment_choice_and_overlap_matches_concatenation() {
        for alignment in 0..8 {
            for divided_len in 0..65 {
                for whole_len in 0..65 {
                    let mut writer = TrialWriter::new();
                    // Sentinels beyond any possible trial remain untouched.
                    writer.bytes = vec![0xa7; 256];
                    let mut prefix = bits(0x96a5, 16);
                    prefix.extend(bits(0x55 & ((1 << alignment) - 1), alignment));
                    write(&mut writer, &prefix);
                    writer.commit_crc(0x1234).unwrap();
                    let start = writer.checkpoint();
                    let divided_bits: Vec<_> = (0..divided_len).map(|i| i % 3 == 0).collect();
                    write(&mut writer, &divided_bits);
                    writer.commit_crc(11).unwrap();
                    writer.commit_crc(22).unwrap();
                    let divided = writer.checkpoint();
                    let whole_start = writer.begin_whole(start, divided).unwrap();
                    let whole_bits: Vec<_> = (0..whole_len).map(|i| i % 5 < 2).collect();
                    write(&mut writer, &whole_bits);
                    let choice = writer.select(start, divided, whole_start, 33).unwrap();
                    let (selected, crcs) = if whole_len < divided_len {
                        assert_eq!(choice, Choice::Whole);
                        (&whole_bits, &[0x1234, 33][..])
                    } else {
                        assert_eq!(choice, Choice::Divided);
                        (&divided_bits, &[0x1234, 11, 22][..])
                    };
                    let expected = [prefix.clone(), selected.clone()].concat();
                    assert_eq!(writer.bits, expected.len());
                    assert_eq!(writer.output(), packed(&expected));
                    assert_eq!(&writer.crcs[..writer.crc_count], crcs);
                    assert_eq!(&writer.bytes[..2], &[0x96, 0xa5]);
                    assert!(writer.bytes[32..].iter().all(|&byte| byte == 0xa7));
                }
            }
        }
    }

    fn fixture(writer: &mut TrialWriter, depth: u8, seed: u32) -> (Vec<bool>, Vec<u32>) {
        let whole: Vec<_> = (0..(seed as usize % 87))
            .map(|i| ((seed >> (i % 32)) & 1) != 0)
            .collect();
        if depth == 0 {
            write(writer, &whole);
            writer.commit_crc(seed).unwrap();
            return (whole, vec![seed]);
        }
        let start = writer.checkpoint();
        let (mut left, mut crcs) =
            fixture(writer, depth - 1, seed.wrapping_mul(17).wrapping_add(1));
        let (right, right_crcs) = fixture(writer, depth - 1, seed.wrapping_mul(23).wrapping_add(2));
        left.extend(right);
        crcs.extend(right_crcs);
        let divided = writer.checkpoint();
        let whole_start = writer.begin_whole(start, divided).unwrap();
        write(writer, &whole);
        writer.select(start, divided, whole_start, seed).unwrap();
        // This oracle constructs independent complete bit sequences and uses
        // no byte cursor, copied range, saved partial byte or writer CRC state.
        if whole.len() < left.len() {
            (whole, vec![seed])
        } else {
            (left, crcs)
        }
    }

    #[test]
    fn nested_trials_restore_parent_prefixes_bytes_and_crc_order() {
        for alignment in 0..8 {
            for seed in 0..48 {
                let mut writer = TrialWriter::new();
                let prefix = bits(0xa5 & ((1 << alignment) - 1), alignment);
                write(&mut writer, &prefix);
                writer.commit_crc(0x8765).unwrap();
                let (encoded, mut crcs) = fixture(&mut writer, 5, seed);
                let expected = [prefix, encoded].concat();
                crcs.insert(0, 0x8765);
                assert_eq!(writer.output(), packed(&expected));
                assert_eq!(writer.bits, expected.len());
                assert_eq!(&writer.crcs[..writer.crc_count], crcs);
            }
        }
    }

    #[test]
    fn appending_a_writer_matches_writing_its_bits_in_place() {
        for alignment in 0..8 {
            for other_len in [0, 1, 7, 8, 9, 31, 32, 33, 64, 65, 1000] {
                let mut writer = TrialWriter::new();
                let prefix: Vec<_> = (0..16 + alignment).map(|i| i % 3 == 1).collect();
                write(&mut writer, &prefix);
                writer.commit_crc(7).unwrap();
                let mut other = TrialWriter::new();
                let tail: Vec<_> = (0..other_len).map(|i| i % 7 < 3).collect();
                write(&mut other, &tail);
                other.commit_crc(8).unwrap();
                other.commit_crc(9).unwrap();
                writer.append(&other).unwrap();
                let expected = [prefix, tail].concat();
                assert_eq!(writer.output(), packed(&expected));
                assert_eq!(writer.bits, expected.len());
                assert_eq!(writer.crcs(), &[7, 8, 9]);
                // Later writes continue from the appended partial byte.
                writer.put(0b101, 3).unwrap();
                let expected = [expected, vec![true, false, true]].concat();
                assert_eq!(writer.output(), packed(&expected));
            }
        }
    }

    #[test]
    fn whole_crc_stays_local_and_storage_failure_preserves_the_cursor() {
        let mut writer = TrialWriter::new();
        let start = writer.checkpoint();
        writer.put(1, 1).unwrap();
        for crc in 0..512 {
            writer.commit_crc(crc).unwrap();
        }
        let divided = writer.checkpoint();
        let whole = writer.begin_whole(start, divided).unwrap();
        assert_eq!(
            writer.select(start, divided, whole, 0x5678).unwrap(),
            Choice::Whole
        );
        assert_eq!(writer.crc_count, 1);
        assert_eq!(writer.crcs[0], 0x5678);
        for _ in 1..MAX_CRCS {
            writer.commit_crc(0).unwrap();
        }
        assert!(writer.commit_crc(0).is_err());
        writer.bytes.truncate(1);
        writer.limit = 1;
        let saved = writer.checkpoint();
        let output = writer.output().to_vec();
        assert!(writer.put(u32::MAX, 32).is_err());
        assert_eq!(writer.bits, saved.bits);
        assert_eq!(writer.output(), output);
        assert!(writer.put(2, 1).is_err());
    }

    #[test]
    fn the_trial_bound_includes_rounding_alignment_and_maximum_passes() {
        use super::MAX_TRIAL_BYTES;
        assert_eq!(MAX_TRIAL_BYTES, 3_897_740);
        // Check the one-block algebra with all rounding terms retained. The
        // recursive induction above adds +1/+2 separately at every level.
        for n in 1usize..=900_000 {
            let symbols = n + 1;
            let header = 6 * (5 + 31 * 258) + 377 + 18;
            let block = (16 * symbols + 6 * symbols.div_ceil(50) + header).div_ceil(8) + 1;
            assert!(block <= (17 * n).div_ceil(8) + 6060);
        }
    }

    #[test]
    fn selecting_a_whole_trial_preflights_crc_space_without_mutation() {
        let mut writer = TrialWriter::new();
        write(&mut writer, &[true, false, true]);
        for crc in 0..MAX_CRCS {
            writer.commit_crc(crc as u32).unwrap();
        }
        let start = writer.checkpoint();
        write(&mut writer, &[false; 19]);
        let divided = writer.checkpoint();
        let whole_start = writer.begin_whole(start, divided).unwrap();
        write(&mut writer, &[true; 5]);
        let bytes = writer.bytes.clone();
        let cursor = writer.checkpoint();
        let crcs = writer.crcs;
        assert!(writer.select(start, divided, whole_start, 0x1234).is_err());
        assert_eq!(writer.bytes, bytes);
        assert_eq!(writer.bits, cursor.bits);
        assert_eq!(writer.checkpoint().partial, cursor.partial);
        assert_eq!(writer.crc_count, cursor.crcs);
        assert_eq!(writer.crcs, crcs);
    }

    #[test]
    fn page_refills_and_non_page_limits_preserve_initialized_bounds_and_errors() {
        for limit in [4095, 4096, 4097, 8193] {
            let mut writer = TrialWriter::new();
            writer.limit = limit;
            let expected: Vec<_> = (0..limit * 8 - 3).map(|i| i % 11 < 5).collect();
            write(&mut writer, &expected);
            assert_eq!(writer.output(), packed(&expected));
            assert_eq!(writer.bytes.len(), limit);
            let cursor = writer.checkpoint();
            let initialized = writer.bytes.clone();
            let pending = writer.pending;
            assert!(writer.put(u32::MAX, 32).is_err());
            assert_eq!(writer.bits, cursor.bits);
            assert_eq!(writer.pending, pending);
            assert_eq!(writer.bytes, initialized);
            writer.put(5, 3).unwrap();
            assert_eq!(writer.bit_len(), limit * 8);
            assert_eq!(writer.bytes.len(), limit);
            assert!(writer.put(0, 1).is_err());
        }
    }

    #[test]
    fn selected_trials_cross_pages_then_append_and_restore_at_every_alignment() {
        for alignment in 0..8 {
            for whole_wins in [false, true] {
                let mut writer = TrialWriter::new();
                let prefix: Vec<_> = (0..4094 * 8 + alignment).map(|i| i % 7 < 3).collect();
                write(&mut writer, &prefix);
                let start = writer.checkpoint();
                let divided_bits: Vec<_> = (0..if whole_wins { 160 } else { 80 })
                    .map(|i| i % 3 == 0)
                    .collect();
                let whole_bits: Vec<_> = (0..if whole_wins { 80 } else { 160 })
                    .map(|i| i % 5 == 0)
                    .collect();
                write(&mut writer, &divided_bits);
                writer.commit_crc(11).unwrap();
                let divided = writer.checkpoint();
                let whole = writer.begin_whole(start, divided).unwrap();
                write(&mut writer, &whole_bits);
                let mut high_water = writer.bits.div_ceil(8);
                writer.select(start, divided, whole, 22).unwrap();
                let mut expected = prefix;
                expected.extend(if whole_wins { whole_bits } else { divided_bits });
                let selected = writer.checkpoint();
                write(&mut writer, &[true; 129]);
                high_water = high_water.max(writer.bits.div_ceil(8));
                writer.restore(selected);
                let tail: Vec<_> = (0..137).map(|i| i % 11 == 0).collect();
                write(&mut writer, &tail);
                high_water = high_water.max(writer.bits.div_ceil(8));
                expected.extend(tail);
                assert_eq!(writer.output(), packed(&expected));
                assert_eq!(writer.bits, expected.len());
                assert_eq!(writer.crcs(), &[if whole_wins { 22 } else { 11 }]);
                assert!(writer.bytes.len() >= high_water);
                assert!(writer.bytes.len() - high_water <= 4095);
            }
        }
    }
}
