//! BZip2 decoding, translated from the 7-Zip BZip2 decoder.
//! Reference: <https://github.com/ip7z/7zip/blob/9128b80e3a471108678e2b3ec3c985861c8d7b0f/CPP/7zip/Compress/BZip2Decoder.cpp>
//! SPDX-License-Identifier: LGPL-2.1-or-later
//! Modified: safe Rust ownership, standard I/O, and bounded reconstruction jobs.
use super::tt_storage::TtBuffer;
use std::io::{self, Read};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

struct Bits<R> {
    reader: R,
    buffer: Box<[u8; 32768]>,
    pos: usize,
    end: usize,
    bits: u64,
    count: u32,
}
impl<R: Read> Bits<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Box::new([0; 32768]),
            pos: 0,
            end: 0,
            bits: 0,
            count: 0,
        }
    }
    fn byte(&mut self) -> io::Result<Option<u8>> {
        if self.pos == self.end {
            self.end = loop {
                match self.reader.read(self.buffer.as_mut_slice()) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    result => break result?,
                }
            };
            self.pos = 0;
            if self.end == 0 {
                return Ok(None);
            }
        }
        let byte = self.buffer[self.pos];
        self.pos += 1;
        Ok(Some(byte))
    }
    #[inline(always)]
    fn peek(&mut self, n: u32) -> io::Result<u32> {
        debug_assert!(n <= 32);
        if n == 0 {
            return Ok(0);
        }
        // CBitDecoder keeps active bits high-aligned. A refill starts with
        // at most 31 bits, so a complete word leaves at most 63 active bits.
        // Unused low bits remain zero after every refill and consume.
        if self.count < n && self.end - self.pos >= 4 {
            let word = u32::from_be_bytes(self.buffer[self.pos..self.pos + 4].try_into().unwrap());
            self.bits |= u64::from(word) << (32 - self.count);
            self.pos += 4;
            self.count += 32;
        }
        while self.count < n {
            let byte = self.byte()?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "truncated BZip2 stream")
            })?;
            self.bits |= u64::from(byte) << (56 - self.count);
            self.count += 8;
        }
        Ok((self.bits >> (64 - n)) as u32)
    }
    #[inline(always)]
    fn take(&mut self, n: u32) -> io::Result<u32> {
        let value = self.peek(n)?;
        self.consume(n);
        Ok(value)
    }
    #[inline(always)]
    fn consume(&mut self, n: u32) {
        debug_assert!(n <= self.count && n <= 32);
        self.bits <<= n;
        self.count -= n;
    }
    fn align(&mut self) {
        self.consume(self.count % 8);
    }
}

struct Huffman {
    first: [u32; 21],
    counts: [u32; 21],
    offsets: [usize; 21],
    symbols: Vec<u16>,
    fast: [u32; 1024],
}
impl Huffman {
    fn new(lengths: &[u8]) -> io::Result<Self> {
        let mut table = Self {
            first: [0; 21],
            counts: [0; 21],
            offsets: [0; 21],
            symbols: Vec::with_capacity(lengths.len()),
            fast: [0; 1024],
        };
        table.rebuild(lengths)?;
        Ok(table)
    }
    fn rebuild(&mut self, lengths: &[u8]) -> io::Result<()> {
        self.first.fill(0);
        self.counts.fill(0);
        self.offsets.fill(0);
        self.fast.fill(0);
        self.symbols.clear();
        self.symbols.reserve_exact(lengths.len());
        for &len in lengths {
            if !(1..=20).contains(&len) {
                return Err(invalid("invalid Huffman code length"));
            }
            self.counts[len as usize] += 1;
        }
        let mut code = 0;
        let mut offset = 0;
        for len in 1..=20 {
            code = (code + self.counts[len - 1]) << 1;
            if code + self.counts[len] > 1 << len {
                return Err(invalid("oversubscribed Huffman tree"));
            }
            self.first[len] = code;
            self.offsets[len] = offset;
            offset += self.counts[len] as usize;
            for (symbol, &symbol_len) in lengths.iter().enumerate() {
                if usize::from(symbol_len) == len {
                    self.symbols.push(symbol as u16);
                }
            }
        }
        for len in 1..=10 {
            for index in 0..self.counts[len] {
                let symbol = self.symbols[self.offsets[len] + index as usize];
                let prefix = ((self.first[len] + index) as usize) << (10 - len);
                for slot in &mut self.fast[prefix..prefix + (1 << (10 - len))] {
                    *slot = (u32::from(symbol) << 8) | len as u32;
                }
            }
        }
        Ok(())
    }
    #[inline(always)]
    fn symbol<R: Read>(&self, bits: &mut Bits<R>) -> io::Result<usize> {
        let entry = self.fast[bits.peek(10)? as usize];
        let len = entry & 255;
        if len != 0 {
            bits.consume(len);
            return Ok((entry >> 8) as usize);
        }
        let mut code = bits.take(10)?;
        for len in 11..=20 {
            code = (code << 1) | bits.take(1)?;
            let index = code.wrapping_sub(self.first[len]);
            if index < self.counts[len] {
                return Ok(usize::from(
                    self.symbols[self.offsets[len] + index as usize],
                ));
            }
        }
        Err(invalid("invalid Huffman code"))
    }
}

#[cfg(test)]
mod high_reservoir_tests {
    use super::{Bits, Huffman, invalid};
    use std::io::{self, Read};
    struct OriginalBits<R> {
        reader: R,
        buffer: Box<[u8; 32768]>,
        pos: usize,
        end: usize,
        bits: u64,
        count: u32,
    }
    impl<R: Read> OriginalBits<R> {
        fn new(reader: R) -> Self {
            Self {
                reader,
                buffer: Box::new([0; 32768]),
                pos: 0,
                end: 0,
                bits: 0,
                count: 0,
            }
        }
        fn byte(&mut self) -> io::Result<Option<u8>> {
            if self.pos == self.end {
                self.end = loop {
                    match self.reader.read(self.buffer.as_mut_slice()) {
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        result => break result?,
                    }
                };
                self.pos = 0;
                if self.end == 0 {
                    return Ok(None);
                }
            }
            let byte = self.buffer[self.pos];
            self.pos += 1;
            Ok(Some(byte))
        }
        #[inline(always)]
        fn peek(&mut self, n: u32) -> io::Result<u32> {
            // All callers request at most 32 bits. A refill starts below that
            // count, so appending a complete word fits the 64-bit reservoir.
            if self.count < n && self.end - self.pos >= 4 {
                let word =
                    u32::from_be_bytes(self.buffer[self.pos..self.pos + 4].try_into().unwrap());
                self.bits = (self.bits << 32) | u64::from(word);
                self.pos += 4;
                self.count += 32;
            }
            while self.count < n {
                let byte = self.byte()?.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "truncated BZip2 stream")
                })?;
                self.bits = (self.bits << 8) | u64::from(byte);
                self.count += 8;
            }
            Ok(((self.bits >> (self.count - n)) & ((1u64 << n) - 1)) as u32)
        }
        #[inline(always)]
        fn take(&mut self, n: u32) -> io::Result<u32> {
            let value = self.peek(n)?;
            self.count -= n;
            Ok(value)
        }
        fn align(&mut self) {
            self.count -= self.count % 8;
        }
    }

    struct OriginalHuffman {
        first: [u32; 21],
        counts: [u32; 21],
        offsets: [usize; 21],
        symbols: Vec<u16>,
        fast: [u32; 1024],
    }
    impl OriginalHuffman {
        fn new(lengths: &[u8]) -> io::Result<Self> {
            let mut table = Self {
                first: [0; 21],
                counts: [0; 21],
                offsets: [0; 21],
                symbols: Vec::with_capacity(lengths.len()),
                fast: [0; 1024],
            };
            for &len in lengths {
                if !(1..=20).contains(&len) {
                    return Err(invalid("invalid Huffman code length"));
                }
                table.counts[len as usize] += 1;
            }
            let mut code = 0;
            let mut offset = 0;
            for len in 1..=20 {
                code = (code + table.counts[len - 1]) << 1;
                if code + table.counts[len] > 1 << len {
                    return Err(invalid("oversubscribed Huffman tree"));
                }
                table.first[len] = code;
                table.offsets[len] = offset;
                offset += table.counts[len] as usize;
                for (symbol, &symbol_len) in lengths.iter().enumerate() {
                    if usize::from(symbol_len) == len {
                        table.symbols.push(symbol as u16);
                    }
                }
            }
            for len in 1..=10 {
                for index in 0..table.counts[len] {
                    let symbol = table.symbols[table.offsets[len] + index as usize];
                    let prefix = ((table.first[len] + index) as usize) << (10 - len);
                    for slot in &mut table.fast[prefix..prefix + (1 << (10 - len))] {
                        *slot = (u32::from(symbol) << 8) | len as u32;
                    }
                }
            }
            Ok(table)
        }
        #[inline(always)]
        fn symbol<R: Read>(&self, bits: &mut OriginalBits<R>) -> io::Result<usize> {
            let entry = self.fast[bits.peek(10)? as usize];
            let len = entry & 255;
            if len != 0 {
                bits.count -= len;
                return Ok((entry >> 8) as usize);
            }
            let mut code = bits.take(10)?;
            for len in 11..=20 {
                code = (code << 1) | bits.take(1)?;
                let index = code.wrapping_sub(self.first[len]);
                if index < self.counts[len] {
                    return Ok(usize::from(
                        self.symbols[self.offsets[len] + index as usize],
                    ));
                }
            }
            Err(invalid("invalid Huffman code"))
        }
    }
    struct Fragmented<'a> {
        bytes: &'a [u8],
        width: usize,
    }
    impl Read for Fragmented<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let n = output.len().min(self.width).min(self.bytes.len());
            output[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }
    fn assert_state<R>(high: &Bits<R>, low: &OriginalBits<R>) {
        assert!(high.count < 64 && low.count < 64);
        let high_active = if high.count == 0 {
            0
        } else {
            high.bits >> (64 - high.count)
        };
        let low_active = low.bits & ((1u64 << low.count) - 1);
        assert_eq!(
            (high.pos, high.end, high.count, high_active),
            (low.pos, low.end, low.count, low_active)
        );
        if high.count == 0 {
            assert_eq!(high.bits, 0);
        } else {
            assert_eq!(high.bits & ((1u64 << (64 - high.count)) - 1), 0);
        }
    }
    fn outcome<T>(result: io::Result<T>) -> Result<T, (io::ErrorKind, String)> {
        result.map_err(|error| (error.kind(), error.to_string()))
    }
    #[test]
    fn high_aligned_huffman_matches_original_symbols_errors_and_refill_edges() {
        let trees = [
            vec![],
            vec![1, 1],
            vec![1, 2, 3, 3],
            vec![8; 256],
            vec![9; 258],
            vec![10; 258],
            vec![20; 258],
            vec![1, 20, 20],
            vec![2, 2],
        ];
        for lengths in trees {
            let packed = Huffman::new(&lengths).unwrap();
            let original = OriginalHuffman::new(&lengths).unwrap();
            assert_eq!(packed.first, original.first);
            assert_eq!(packed.counts, original.counts);
            assert_eq!(packed.offsets, original.offsets);
            assert_eq!(packed.symbols, original.symbols);
            assert_eq!(packed.fast, original.fast);
            let mut words = vec![0u32, u32::MAX, 0xaaaa_aaaa, 0x5555_5555];
            for length in 1..=20 {
                for index in 0..packed.counts[length] {
                    words.push((packed.first[length] + index) << (32 - length));
                }
            }
            for word in words {
                let mut bytes = word.to_be_bytes().to_vec();
                bytes.push(0xa5);
                for end in 0..=bytes.len() {
                    for width in [1, 3, 4, 32768] {
                        let mut left = Bits::new(Fragmented {
                            bytes: &bytes[..end],
                            width,
                        });
                        let mut right = OriginalBits::new(Fragmented {
                            bytes: &bytes[..end],
                            width,
                        });
                        assert_eq!(
                            outcome(packed.symbol(&mut left)),
                            outcome(original.symbol(&mut right))
                        );
                        assert_state(&left, &right);
                        assert_eq!(left.reader.bytes.len(), right.reader.bytes.len());
                    }
                }
            }
            let mut encoded = Vec::new();
            let mut symbols = Vec::new();
            let mut used = 0u8;
            for length in 1..=20 {
                for index in 0..packed.counts[length] {
                    let code = packed.first[length] + index;
                    symbols.push(usize::from(
                        packed.symbols[packed.offsets[length] + index as usize],
                    ));
                    for bit in (0..length).rev() {
                        if used == 0 {
                            encoded.push(0u8);
                        }
                        *encoded.last_mut().unwrap() |= (((code >> bit) & 1) as u8) << (7 - used);
                        used = (used + 1) & 7;
                    }
                }
            }
            encoded.extend([0; 4]);
            for width in [1, 3, 4, 32768] {
                let mut left = Bits::new(Fragmented {
                    bytes: &encoded,
                    width,
                });
                let mut right = OriginalBits::new(Fragmented {
                    bytes: &encoded,
                    width,
                });
                for &symbol in &symbols {
                    assert_eq!(packed.symbol(&mut left).unwrap(), symbol);
                    assert_eq!(original.symbol(&mut right).unwrap(), symbol);
                    assert_state(&left, &right);
                    assert_eq!(left.reader.bytes.len(), right.reader.bytes.len());
                }
            }
        }
        for lengths in [vec![0], vec![21], vec![1, 1, 1]] {
            assert_eq!(
                Huffman::new(&lengths)
                    .err()
                    .map(|e| (e.kind(), e.to_string())),
                OriginalHuffman::new(&lengths)
                    .err()
                    .map(|e| (e.kind(), e.to_string()))
            );
        }
    }
    struct Faulted<'a> {
        bytes: &'a [u8],
        width: usize,
        calls: usize,
        fault: Option<(usize, io::ErrorKind)>,
    }
    impl Read for Faulted<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            if let Some((_, kind)) = self.fault.filter(|(call, _)| self.calls == *call) {
                return Err(io::Error::new(kind, "fixture read fault"));
            }
            let n = output.len().min(self.width).min(self.bytes.len());
            output[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }
    #[test]
    fn high_aligned_bits_match_original_peek_take_align_partial_errors_and_zero() {
        for length in [0, 1, 2, 3, 4, 5, 31, 32, 33, 257, 32775] {
            let bytes: Vec<u8> = (0..length).map(|i| ((i * 97) ^ (i >> 3)) as u8).collect();
            for width in [1, 3, 4, 17, 32768] {
                for fault in [
                    None,
                    Some((1, io::ErrorKind::WouldBlock)),
                    Some((2, io::ErrorKind::Interrupted)),
                ] {
                    let input = || Faulted {
                        bytes: &bytes,
                        width,
                        calls: 0,
                        fault,
                    };
                    let mut high = Bits::new(input());
                    let mut low = OriginalBits::new(input());
                    let mut random = 0x1234_5678u32;
                    for turn in 0..(length * 4 + 128) {
                        random ^= random << 13;
                        random ^= random >> 17;
                        random ^= random << 5;
                        let n = random % 33;
                        let got = if turn % 5 == 0 {
                            high.align();
                            Ok(0)
                        } else if turn % 3 == 0 {
                            high.peek(n)
                        } else {
                            high.take(n)
                        };
                        let want = if turn % 5 == 0 {
                            low.align();
                            Ok(0)
                        } else if turn % 3 == 0 {
                            low.peek(n)
                        } else {
                            low.take(n)
                        };
                        let ended = got
                            .as_ref()
                            .err()
                            .is_some_and(|e| e.kind() == io::ErrorKind::UnexpectedEof);
                        assert_eq!(outcome(got), outcome(want));
                        assert_state(&high, &low);
                        assert_eq!(
                            (high.reader.bytes.len(), high.reader.calls),
                            (low.reader.bytes.len(), low.reader.calls)
                        );
                        assert_eq!(high.peek(0).unwrap(), 0);
                        assert_eq!(low.peek(0).unwrap(), 0);
                        assert_state(&high, &low);
                        if ended {
                            break;
                        }
                    }
                }
            }
        }
    }
}

struct Block {
    last: Vec<u8>,
    counts: [usize; 256],
    origin: usize,
    randomized: bool,
    crc: u32,
}
// CBase retains entropy scratch between blocks; published Blocks own no borrows.
#[derive(Default)]
struct ParserWorkspace {
    alphabet: Vec<u8>,
    selectors: Vec<u8>,
    lengths: Vec<u8>,
    tables: Vec<Huffman>,
}

// Keep the large cold return temporary out of Parser::next.
#[inline(never)]
fn append_huffman(tables: &mut Vec<Huffman>, lengths: &[u8]) -> io::Result<()> {
    tables.push(Huffman::new(lengths)?);
    Ok(())
}

struct Parser<R> {
    bits: Bits<R>,
    workspace: ParserWorkspace,
    limit: usize,
    combined: u32,
    started: bool,
    finished: bool,
    multistream: bool,
}
impl<R: Read> Parser<R> {
    fn new(reader: R) -> Self {
        Self {
            bits: Bits::new(reader),
            workspace: ParserWorkspace::default(),
            limit: 0,
            combined: 0,
            started: false,
            finished: false,
            multistream: true,
        }
    }
    fn header(&mut self, first: bool) -> io::Result<bool> {
        self.bits.align();
        let byte = if self.bits.count >= 8 {
            Some(self.bits.take(8)? as u8)
        } else {
            self.bits.byte()?
        };
        let Some(byte) = byte else {
            if first {
                return Err(invalid("missing BZip2 header"));
            }
            return Ok(false);
        };
        if byte != b'B' || self.bits.take(16)? != u32::from_be_bytes([0, 0, b'Z', b'h']) {
            return Err(invalid("invalid BZip2 header"));
        }
        let digit = self.bits.take(8)?;
        if !(49..=57).contains(&digit) {
            return Err(invalid("invalid BZip2 block size"));
        }
        self.limit = (digit as usize - 48) * 100_000;
        self.combined = 0;
        self.started = true;
        Ok(true)
    }
    fn next(&mut self) -> io::Result<Option<Block>> {
        if self.finished {
            return Ok(None);
        }
        if !self.started {
            self.header(true)?;
        }
        loop {
            let magic = (u64::from(self.bits.take(24)?) << 24) | u64::from(self.bits.take(24)?);
            if magic == 0x177245385090 {
                if self.bits.take(32)? != self.combined {
                    return Err(invalid("BZip2 combined CRC mismatch"));
                }
                if !self.multistream {
                    self.bits.align();
                    self.finished = true;
                    return Ok(None);
                }
                if !self.header(false)? {
                    self.finished = true;
                    return Ok(None);
                }
                continue;
            }
            if magic != 0x314159265359 {
                return Err(invalid("invalid BZip2 block marker"));
            }
            let crc = self.bits.take(32)?;
            let randomized = self.bits.take(1)? != 0;
            let origin = self.bits.take(24)? as usize;
            let groups = self.bits.take(16)?;
            let alphabet = &mut self.workspace.alphabet;
            alphabet.clear();
            alphabet.reserve_exact(256);
            for group in 0..16 {
                if groups & (1 << (15 - group)) != 0 {
                    let mask = self.bits.take(16)?;
                    for low in 0..16 {
                        if mask & (1 << (15 - low)) != 0 {
                            alphabet.push((group * 16 + low) as u8);
                        }
                    }
                }
            }
            if alphabet.is_empty() {
                return Err(invalid("empty BZip2 alphabet"));
            }
            let alphabet_size = alphabet.len() + 2;
            let table_count = self.bits.take(3)? as usize;
            let selector_count = self.bits.take(15)? as usize;
            if !(2..=6).contains(&table_count) || selector_count == 0 {
                return Err(invalid("invalid BZip2 Huffman groups"));
            }
            let mut selector_mtf = [0u8, 1, 2, 3, 4, 5];
            let selectors = &mut self.workspace.selectors;
            selectors.clear();
            selectors.reserve_exact(selector_count.min(18002));
            for _ in 0..selector_count {
                let mut index = 0;
                while self.bits.take(1)? != 0 {
                    index += 1;
                    if index >= table_count {
                        return Err(invalid("invalid BZip2 selector"));
                    }
                }
                let selected = selector_mtf[index];
                selector_mtf[..=index].rotate_right(1);
                if selectors.len() < 18002 {
                    selectors.push(selected);
                }
            }
            let tables = &mut self.workspace.tables;
            tables.reserve_exact(table_count.saturating_sub(tables.len()));
            for table_index in 0..table_count {
                let mut length = self.bits.take(5)? as i32;
                if !(1..=20).contains(&length) {
                    return Err(invalid("invalid Huffman length"));
                }
                let lengths = &mut self.workspace.lengths;
                lengths.clear();
                lengths.reserve_exact(alphabet_size);
                for _ in 0..alphabet_size {
                    while self.bits.take(1)? != 0 {
                        length += if self.bits.take(1)? == 0 { 1 } else { -1 };
                        if !(1..=20).contains(&length) {
                            return Err(invalid("invalid Huffman length delta"));
                        }
                    }
                    if !(1..=20).contains(&length) {
                        return Err(invalid("invalid Huffman length"));
                    }
                    lengths.push(length as u8);
                }
                if table_index == tables.len() {
                    append_huffman(tables, lengths)?;
                } else {
                    tables[table_index].rebuild(lengths)?;
                }
            }
            #[cfg(target_arch = "x86_64")]
            let mut mtf = {
                let mut bytes = [0u8; 256];
                bytes[..alphabet.len()].copy_from_slice(&alphabet);
                bytes
            };
            #[cfg(not(target_arch = "x86_64"))]
            let mut mtf = {
                let mut words = [0u64; 32];
                for (index, &byte) in alphabet.iter().enumerate() {
                    words[index / 8] |= u64::from(byte) << ((index & 7) * 8);
                }
                words
            };
            let mut counts = [0usize; 256];
            let mut last = Vec::with_capacity(self.limit);
            let mut selector_index = 0;
            let mut group_left = 0;
            let mut huffman = &tables[0];
            let mut run = 0usize;
            let mut weight = 1usize;
            loop {
                if group_left == 0 {
                    let selector = *selectors
                        .get(selector_index)
                        .ok_or_else(|| invalid("missing BZip2 selector"))?;
                    huffman = &tables[usize::from(selector)];
                    selector_index += 1;
                    group_left = 50;
                }
                let symbol = huffman.symbol(&mut self.bits)?;
                group_left -= 1;
                if symbol <= 1 {
                    run = run
                        .checked_add(weight * (symbol + 1))
                        .ok_or_else(|| invalid("BZip2 run overflow"))?;
                    if run > self.limit - last.len() {
                        return Err(invalid("BZip2 block exceeds declared size"));
                    }
                    weight = weight
                        .checked_mul(2)
                        .ok_or_else(|| invalid("BZip2 run overflow"))?;
                    continue;
                }
                if run != 0 {
                    #[cfg(target_arch = "x86_64")]
                    let byte = mtf[0];
                    #[cfg(not(target_arch = "x86_64"))]
                    let byte = mtf[0] as u8;
                    counts[byte as usize] += run;
                    last.resize(last.len() + run, byte);
                    run = 0;
                    weight = 1;
                }
                if symbol == alphabet_size - 1 {
                    break;
                }
                let index = symbol - 1;
                if index >= alphabet.len() {
                    return Err(invalid("invalid BZip2 MTF index"));
                }
                #[cfg(target_arch = "x86_64")]
                let byte = mtf[index];
                #[cfg(not(target_arch = "x86_64"))]
                let (next, byte) = {
                    let next = mtf[index / 8];
                    (next, (next >> ((index & 7) * 8)) as u8)
                };
                if last.len() == self.limit {
                    return Err(invalid("BZip2 block exceeds declared size"));
                }
                // ReadBlock2 publishes the byte and histogram before moving MTF words.
                last.push(byte);
                counts[byte as usize] += 1;
                #[cfg(target_arch = "x86_64")]
                move_mtf_bytes(&mut mtf, index, byte);
                #[cfg(not(target_arch = "x86_64"))]
                move_mtf(&mut mtf, index, next, byte);
            }
            if origin >= last.len() {
                return Err(invalid("invalid BZip2 BWT origin"));
            }
            self.combined = self.combined.rotate_left(1) ^ crc;
            return Ok(Some(Block {
                last,
                counts,
                origin,
                randomized,
                crc,
            }));
        }
    }
}

// CMtf8Decoder::GetAndMove with byte storage: short prefixes use paired word
// carries; longer prefixes use the platform's overlapping copy. The selected
// byte is saved before either path mutates the prefix.
#[cfg(any(target_arch = "x86_64", test))]
#[inline(always)]
fn move_mtf_bytes(bytes: &mut [u8; 256], index: usize, byte: u8) {
    if index < 32 {
        let (words, _) = bytes.as_chunks_mut::<8>();
        let word = index / 8;
        let next = u64::from_le_bytes(words[word]);
        let mut carry = u64::from(byte);
        let (pairs, tail) = words[..word].as_chunks_mut::<2>();
        for pair in pairs {
            let first = u64::from_le_bytes(pair[0]);
            let second = u64::from_le_bytes(pair[1]);
            pair[0] = ((first << 8) | carry).to_le_bytes();
            pair[1] = ((second << 8) | (first >> 56)).to_le_bytes();
            carry = second >> 56;
        }
        for entry in tail {
            let previous = u64::from_le_bytes(*entry);
            *entry = ((previous << 8) | carry).to_le_bytes();
            carry = previous >> 56;
        }
        let mask = u64::MAX >> (56 - (index & 7) * 8);
        words[word] = ((next & !mask) | (((next << 8) | carry) & mask)).to_le_bytes();
        return;
    }
    bytes.copy_within(..index, 1);
    bytes[0] = byte;
}

// CMtf8Decoder::GetAndMove, with the selected byte already extracted.
#[cfg(any(not(target_arch = "x86_64"), test))]
#[inline(always)]
fn move_mtf(words: &mut [u64; 32], index: usize, next: u64, byte: u8) {
    #[cfg(target_arch = "x86_64")]
    move_mtf_reverse(words, index, next, byte);
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    move_mtf_forward(words, index, next, byte);
    #[cfg(all(
        not(target_arch = "x86_64"),
        not(all(target_os = "linux", target_arch = "aarch64"))
    ))]
    move_mtf_paired(words, index, next, byte);
}

// ReadBlock2 / CMtf8Decoder::GetAndMove advances one word per carry step.
// Keep the paired implementation selected on its existing other platforms.
#[cfg(any(all(target_os = "linux", target_arch = "aarch64"), test))]
#[inline(always)]
fn move_mtf_forward(words: &mut [u64; 32], index: usize, next: u64, byte: u8) {
    let word = index / 8;
    let shift = (index & 7) * 8;
    let mut carry = u64::from(byte);
    for entry in &mut words[..word] {
        let previous = *entry;
        *entry = (previous << 8) | carry;
        carry = previous >> 56;
    }
    let mask = u64::MAX >> (56 - shift);
    words[word] = (next & !mask) | (((next << 8) | carry) & mask);
}

#[cfg(any(
    all(
        not(target_arch = "x86_64"),
        not(all(target_os = "linux", target_arch = "aarch64"))
    ),
    test
))]
#[inline(always)]
fn move_mtf_paired(words: &mut [u64; 32], index: usize, next: u64, byte: u8) {
    let word = index / 8;
    let shift = (index & 7) * 8;
    let mut carry = u64::from(byte);
    let (pairs, tail) = words[..word].as_chunks_mut::<2>();
    for pair in pairs {
        let [first, second] = *pair;
        pair[0] = (first << 8) | carry;
        pair[1] = (second << 8) | (first >> 56);
        carry = second >> 56;
    }
    for entry in tail {
        let previous = *entry;
        *entry = (previous << 8) | carry;
        carry = previous >> 56;
    }
    let mask = u64::MAX >> (56 - shift);
    words[word] = (next & !mask) | (((next << 8) | carry) & mask);
}

// CMtf8Decoder::GetAndMove: reverse traversal keeps neighboring source words
// unchanged until they are consumed, allowing independent shifts on x86-64.
#[cfg(test)]
#[inline(always)]
fn move_mtf_reverse(words: &mut [u64; 32], index: usize, next: u64, byte: u8) {
    let word = index / 8;
    let shift = (index & 7) * 8;
    let mask = u64::MAX >> (56 - shift);
    if word == 0 {
        words[0] = (next & !mask) | (((next << 8) | u64::from(byte)) & mask);
        return;
    }
    let carry = words[word - 1] >> 56;
    words[word] = (next & !mask) | (((next << 8) | carry) & mask);
    for position in (1..word).rev() {
        let current = words[position];
        let previous = words[position - 1];
        words[position] = (current << 8) | (previous >> 56);
    }
    words[0] = (words[0] << 8) | u64::from(byte);
}

// The fixed randomization sequence is part of the legacy wire format.
const RANDOM_NUMBERS: [i32; 512] = [
    619, 720, 127, 481, 931, 816, 813, 233, 566, 247, 985, 724, 205, 454, 863, 491, 741, 242, 949,
    214, 733, 859, 335, 708, 621, 574, 73, 654, 730, 472, 419, 436, 278, 496, 867, 210, 399, 680,
    480, 51, 878, 465, 811, 169, 869, 675, 611, 697, 867, 561, 862, 687, 507, 283, 482, 129, 807,
    591, 733, 623, 150, 238, 59, 379, 684, 877, 625, 169, 643, 105, 170, 607, 520, 932, 727, 476,
    693, 425, 174, 647, 73, 122, 335, 530, 442, 853, 695, 249, 445, 515, 909, 545, 703, 919, 874,
    474, 882, 500, 594, 612, 641, 801, 220, 162, 819, 984, 589, 513, 495, 799, 161, 604, 958, 533,
    221, 400, 386, 867, 600, 782, 382, 596, 414, 171, 516, 375, 682, 485, 911, 276, 98, 553, 163,
    354, 666, 933, 424, 341, 533, 870, 227, 730, 475, 186, 263, 647, 537, 686, 600, 224, 469, 68,
    770, 919, 190, 373, 294, 822, 808, 206, 184, 943, 795, 384, 383, 461, 404, 758, 839, 887, 715,
    67, 618, 276, 204, 918, 873, 777, 604, 560, 951, 160, 578, 722, 79, 804, 96, 409, 713, 940,
    652, 934, 970, 447, 318, 353, 859, 672, 112, 785, 645, 863, 803, 350, 139, 93, 354, 99, 820,
    908, 609, 772, 154, 274, 580, 184, 79, 626, 630, 742, 653, 282, 762, 623, 680, 81, 927, 626,
    789, 125, 411, 521, 938, 300, 821, 78, 343, 175, 128, 250, 170, 774, 972, 275, 999, 639, 495,
    78, 352, 126, 857, 956, 358, 619, 580, 124, 737, 594, 701, 612, 669, 112, 134, 694, 363, 992,
    809, 743, 168, 974, 944, 375, 748, 52, 600, 747, 642, 182, 862, 81, 344, 805, 988, 739, 511,
    655, 814, 334, 249, 515, 897, 955, 664, 981, 649, 113, 974, 459, 893, 228, 433, 837, 553, 268,
    926, 240, 102, 654, 459, 51, 686, 754, 806, 760, 493, 403, 415, 394, 687, 700, 946, 670, 656,
    610, 738, 392, 760, 799, 887, 653, 978, 321, 576, 617, 626, 502, 894, 679, 243, 440, 680, 879,
    194, 572, 640, 724, 926, 56, 204, 700, 707, 151, 457, 449, 797, 195, 791, 558, 945, 679, 297,
    59, 87, 824, 713, 663, 412, 693, 342, 606, 134, 108, 571, 364, 631, 212, 174, 643, 304, 329,
    343, 97, 430, 751, 497, 314, 983, 374, 822, 928, 140, 206, 73, 263, 980, 736, 876, 478, 430,
    305, 170, 514, 364, 692, 829, 82, 855, 953, 676, 246, 369, 970, 294, 750, 807, 827, 150, 790,
    288, 923, 804, 378, 215, 828, 592, 281, 565, 555, 710, 82, 896, 831, 547, 261, 524, 462, 293,
    465, 502, 56, 661, 821, 976, 991, 658, 869, 905, 758, 745, 193, 768, 550, 608, 933, 378, 286,
    215, 979, 792, 961, 61, 688, 793, 644, 986, 403, 106, 366, 905, 644, 372, 567, 466, 434, 645,
    210, 389, 550, 919, 135, 780, 773, 635, 389, 707, 100, 626, 958, 165, 504, 920, 176, 193, 713,
    857, 265, 203, 50, 668, 108, 645, 990, 626, 197, 510, 357, 358, 850, 858, 364, 936, 638,
];

// DecodeBlock1 and CSpecState::Decode: compact and paired TT words share the
// scalar reconstruction state; their concrete kernels contain no mode dispatch.
struct OutputState<W: super::tt_storage::Word = u32> {
    links: TtBuffer<W>,
    position: u32,
    remaining: usize,
    previous: u8,
    repeated: u8,
    pending: usize,
    random_index: usize,
    random_count: i32,
    randomized: bool,
    crc: u32,
    expected_crc: u32,
    delivered: u64,
}
mod compact;
#[cfg(test)]
mod mode_tests;
mod paired;
mod tt;

impl<W: super::tt_storage::Word> OutputState<W> {
    fn finished(&self) -> bool {
        self.remaining == 0 && self.pending == 0
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Compact,
    Paired,
}
impl Mode {
    const fn platform() -> Self {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Self::Paired
        } else {
            Self::Compact
        }
    }
}
enum Links {
    Compact(TtBuffer<u32>),
    Paired(TtBuffer<[u32; 2]>),
}
impl Links {
    fn new(mode: Mode) -> Self {
        match mode {
            Mode::Compact => Self::Compact(TtBuffer::default()),
            Mode::Paired => Self::Paired(TtBuffer::default()),
        }
    }
    fn take(&mut self) -> Self {
        match self {
            Self::Compact(words) => Self::Compact(std::mem::take(words)),
            Self::Paired(words) => Self::Paired(std::mem::take(words)),
        }
    }
}
enum BlockOutput {
    Compact(OutputState<u32>),
    Paired(OutputState<[u32; 2]>),
}
impl BlockOutput {
    fn new(block: Block, links: Links) -> Self {
        match links {
            Links::Compact(words) => Self::Compact(OutputState::<u32>::new(block, words)),
            Links::Paired(words) => Self::Paired(OutputState::<[u32; 2]>::new(block, words)),
        }
    }
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Compact(output) => output.read(bytes),
            Self::Paired(output) => output.read(bytes),
        }
    }
    fn delivered(&self) -> u64 {
        match self {
            Self::Compact(output) => output.delivered,
            Self::Paired(output) => output.delivered,
        }
    }
    fn finished(&self) -> bool {
        match self {
            Self::Compact(output) => output.finished(),
            Self::Paired(output) => output.finished(),
        }
    }
    fn into_links(self) -> Links {
        match self {
            Self::Compact(output) => Links::Compact(output.links),
            Self::Paired(output) => Links::Paired(output.links),
        }
    }
}

type OutputCallback = Arc<Mutex<Box<dyn FnMut(&[u8]) + Send>>>;
fn observe_output(callback: &Option<OutputCallback>, bytes: &[u8]) -> io::Result<()> {
    if let Some(callback) = callback {
        let mut callback = callback
            .lock()
            .map_err(|_| io::Error::other("BZip2 output callback poisoned"))?;
        callback(bytes);
    }
    Ok(())
}

const OUTPUT_CHUNK: usize = 65536;
const OUTPUT_OWNERS: usize = 18;
#[cfg(test)]
mod pool_tests;

struct OutputPool {
    returned: std::sync::mpsc::Receiver<Vec<u8>>,
    allocated: usize,
}
impl OutputPool {
    fn take(&mut self) -> Option<Vec<u8>> {
        use std::sync::mpsc::TryRecvError;
        let mut bytes = match self.returned.try_recv() {
            Ok(bytes) => bytes,
            Err(TryRecvError::Disconnected) => return None,
            Err(TryRecvError::Empty) if self.allocated < OUTPUT_OWNERS => {
                self.allocated += 1;
                Vec::with_capacity(OUTPUT_CHUNK)
            }
            Err(TryRecvError::Empty) => self.returned.recv().ok()?,
        };
        bytes.resize(OUTPUT_CHUNK, 0);
        Some(bytes)
    }
}

enum WorkerMessage {
    Data(Vec<u8>),
    Finished,
    Error(io::Error),
}
struct Worker {
    jobs: Option<std::sync::mpsc::SyncSender<Block>>,
    receiver: Option<std::sync::mpsc::Receiver<WorkerMessage>>,
    returned: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    handle: Option<JoinHandle<()>>,
}
impl Worker {
    fn start(callback: Option<OutputCallback>, mode: Mode) -> io::Result<Self> {
        let (jobs, blocks) = std::sync::mpsc::sync_channel(1);
        // The 1 MiB ring lets a normal block finish while the caller parses
        // its successor, without expanding an RLE-heavy block into one allocation.
        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        let (returned, buffers) = std::sync::mpsc::sync_channel(OUTPUT_OWNERS);
        let handle = std::thread::Builder::new()
            .name("bzip2-reconstruct".into())
            .spawn(move || {
                let mut links = Links::new(mode);
                let mut pool = OutputPool {
                    returned: buffers,
                    allocated: 0,
                };
                while let Ok(block) = blocks.recv() {
                    let mut output = BlockOutput::new(block, links.take());
                    loop {
                        let Some(mut bytes) = pool.take() else {
                            return;
                        };
                        match output.read(&mut bytes) {
                            Ok(n) => {
                                bytes.truncate(n);
                                if let Err(error) = observe_output(&callback, &bytes) {
                                    let _ = sender.send(WorkerMessage::Error(error));
                                    return;
                                }
                                if sender.send(WorkerMessage::Data(bytes)).is_err() {
                                    return;
                                }
                                if output.finished() {
                                    if sender.send(WorkerMessage::Finished).is_err() {
                                        return;
                                    }
                                    break;
                                }
                            }
                            Err(error) => {
                                let _ = sender.send(WorkerMessage::Error(error));
                                return;
                            }
                        }
                    }
                    links = output.into_links();
                }
            })?;
        Ok(Self {
            jobs: Some(jobs),
            receiver: Some(receiver),
            returned: Some(returned),
            handle: Some(handle),
        })
    }
    fn recycle(&self, bytes: Vec<u8>) {
        if bytes.capacity() >= OUTPUT_CHUNK {
            // Each unique owner returns at most once. This never waits for a
            // worker that may itself be blocked on the forward queue.
            let result = self
                .returned
                .as_ref()
                .expect("live return queue")
                .try_send(bytes);
            debug_assert!(
                !matches!(result, Err(std::sync::mpsc::TrySendError::Full(_))),
                "output owner returned twice or return queue undersized"
            );
        }
    }
    fn submit(&self, block: Block) -> io::Result<()> {
        self.jobs
            .as_ref()
            .expect("live worker job queue")
            .send(block)
            .map_err(|_| io::Error::other("BZip2 worker stopped before accepting a block"))
    }
    fn receive(&mut self) -> io::Result<WorkerMessage> {
        match self.receiver.as_ref().expect("live worker receiver").recv() {
            Ok(message) => Ok(message),
            Err(_) => {
                let panicked = self
                    .handle
                    .take()
                    .is_some_and(|handle| handle.join().is_err());
                Err(io::Error::other(if panicked {
                    "BZip2 worker panicked"
                } else {
                    "BZip2 worker stopped before finishing a block"
                }))
            }
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Close every channel before joining: the worker can wait for output
        // delivery, a returned owner, or the next block.
        self.receiver.take();
        self.returned.take();
        self.jobs.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// A bounded, block-streaming BZip2 decoder, including concatenated streams.
/// Thread counts default to one. As in the reference decoder, counts above one
/// select a pipeline overlapping entropy parsing with block reconstruction.
pub struct Decoder<R: Read> {
    parser: Parser<R>,
    mode: Mode,
    threads: usize,
    worker: Option<Worker>,
    worker_active: bool,
    staged: Option<Block>,
    active: Option<BlockOutput>,
    output: Vec<u8>,
    output_end: usize,
    position: usize,
    error: Option<io::Error>,
    failed: bool,
    callback: Option<OutputCallback>,
}
impl<R: Read> Decoder<R> {
    /// Creates a single-threaded decoder.
    pub fn new(reader: R) -> Self {
        Self::with_threads(reader, 1)
    }
    /// Creates a decoder with a bounded reconstruction thread budget.
    pub fn with_threads(reader: R, threads: usize) -> Self {
        Self::with_mode(reader, threads, Mode::platform())
    }
    /// Creates a compact decoder for an explicitly constrained memory budget.
    ///
    /// This selects one 32-bit TT word per raw symbol on every platform. The
    /// ordinary constructors select paired words only on macOS ARM64.
    pub fn with_low_memory(reader: R, threads: usize) -> Self {
        Self::with_mode(reader, threads, Mode::Compact)
    }
    fn with_mode(reader: R, threads: usize, mode: Mode) -> Self {
        Self {
            parser: Parser::new(reader),
            mode,
            threads: if cfg!(target_family = "wasm") {
                1
            } else {
                threads.clamp(1, 2)
            },
            worker: None,
            worker_active: false,
            staged: None,
            active: None,
            output: Vec::new(),
            output_end: 0,
            position: 0,
            error: None,
            failed: false,
            callback: None,
        }
    }
    /// Observes each reconstructed output chunk before it is queued for delivery.
    ///
    /// With multiple threads the callback runs on the reconstruction worker;
    /// single-threaded decoding invokes it on the caller. It may observe read-ahead
    /// bytes before they are delivered, including bytes discarded on early drop.
    pub fn with_output_callback(
        reader: R,
        threads: usize,
        callback: impl FnMut(&[u8]) + Send + 'static,
    ) -> Self {
        let mut decoder = Self::with_threads(reader, threads);
        decoder.callback = Some(Arc::new(Mutex::new(Box::new(callback))));
        decoder
    }
    /// Combines the compact memory mode with producer-side output observation.
    /// Callback and read-ahead behavior matches [`Self::with_output_callback`].
    pub fn with_low_memory_output_callback(
        reader: R,
        threads: usize,
        callback: impl FnMut(&[u8]) + Send + 'static,
    ) -> Self {
        let mut decoder = Self::with_low_memory(reader, threads);
        decoder.callback = Some(Arc::new(Mutex::new(Box::new(callback))));
        decoder
    }
    /// Selects whether concatenated streams are decoded (enabled by default).
    ///
    /// Configure this before the first nonempty read: the parallel pipeline can
    /// parse future stream trailers ahead of output delivery. Disabling it stops
    /// after the first validated stream trailer and ignores subsequent padding.
    /// This does not reset a decoder that has already finished.
    pub fn set_multistream(&mut self, multistream: bool) {
        self.parser.multistream = multistream;
    }
    /// Conservative peak codec allocation, excluding the reader, OS thread stacks,
    /// and allocations owned or performed by an output callback.
    pub const fn memory_usage(threads: usize) -> u64 {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            if threads > 1 {
                16 * 1024 * 1024
            } else {
                10 * 1024 * 1024
            }
        } else {
            Self::low_memory_usage(threads)
        }
    }
    /// Conservative allocation bound for [`Self::with_low_memory`], with the
    /// same reader, stack and callback exclusions as [`Self::memory_usage`].
    pub const fn low_memory_usage(threads: usize) -> u64 {
        if threads > 1 && !cfg!(target_family = "wasm") {
            12 * 1024 * 1024
        } else {
            6 * 1024 * 1024
        }
    }
    /// Returns the input reader. Buffered compressed bytes are discarded.
    pub fn into_inner(mut self) -> R {
        self.worker.take();
        self.parser.bits.reader
    }
    /// Borrows the input reader.
    pub fn get_ref(&self) -> &R {
        &self.parser.bits.reader
    }
    /// Mutably borrows the input reader; changing its position invalidates decoding.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.parser.bits.reader
    }
    fn next_output(&mut self) -> io::Result<bool> {
        if self.threads == 1 {
            if self.active.as_ref().is_none_or(BlockOutput::finished) {
                let links = self
                    .active
                    .take()
                    .map_or_else(|| Links::new(self.mode), BlockOutput::into_links);
                self.active = match self.parser.next()? {
                    Some(block) => Some(BlockOutput::new(block, links)),
                    None => return Ok(false),
                };
            }
            // Match CDecoder's 1 MiB buffer for unobserved ST
            // output; preserve callback chunk boundaries and worker buffers.
            let chunk = if self.callback.is_some() {
                OUTPUT_CHUNK
            } else {
                1 << 20
            };
            self.output.resize(chunk, 0);
            let n = self
                .active
                .as_mut()
                .expect("active block")
                .read(&mut self.output)?;
            self.output_end = n;
            if self.callback.is_some() {
                // A callback panic may be caught by the caller. Keep that decoder
                // failed so its unobserved buffered bytes cannot then be delivered.
                self.failed = true;
                observe_output(&self.callback, &self.output[..n])?;
                self.failed = false;
            }
            self.position = 0;
            return Ok(true);
        }
        if let Some(worker) = &self.worker {
            worker.recycle(std::mem::take(&mut self.output));
            self.output_end = 0;
            self.position = 0;
        }
        loop {
            if !self.worker_active {
                let block = if let Some(block) = self.staged.take() {
                    Some(block)
                } else if self.error.is_none() && !self.parser.finished {
                    match self.parser.next() {
                        Ok(block) => block,
                        Err(error) => {
                            self.error = Some(error);
                            None
                        }
                    }
                } else {
                    None
                };
                if let Some(block) = block {
                    if self.worker.is_none() {
                        self.worker = Some(Worker::start(self.callback.clone(), self.mode)?);
                    }
                    self.worker
                        .as_ref()
                        .expect("started worker")
                        .submit(block)?;
                    self.worker_active = true;
                }
            }
            // RunScout / DecodeStreams overlap exactly one entropy parser and
            // one reconstruction worker, irrespective of a larger thread request.
            if self.worker_active
                && self.staged.is_none()
                && self.error.is_none()
                && !self.parser.finished
            {
                match self.parser.next() {
                    Ok(block) => self.staged = block,
                    Err(error) => self.error = Some(error),
                }
            }
            if self.worker_active {
                match self.worker.as_mut().expect("active worker").receive()? {
                    WorkerMessage::Data(bytes) => {
                        self.output_end = bytes.len();
                        self.output = bytes;
                        self.position = 0;
                        return Ok(true);
                    }
                    WorkerMessage::Finished => {
                        self.worker_active = false;
                        continue;
                    }
                    WorkerMessage::Error(error) => return Err(error),
                }
            }
            if let Some(error) = self.error.take() {
                return Err(error);
            }
            self.worker.take();
            return Ok(false);
        }
    }
}
impl<R: Read> Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.failed {
            return Err(invalid("BZip2 decoder is in a failed state"));
        }
        while self.position == self.output_end {
            match self.next_output() {
                Ok(true) => {}
                Ok(false) => return Ok(0),
                Err(error) => {
                    self.failed = true;
                    self.worker.take();
                    return Err(error);
                }
            }
        }
        let count = buf.len().min(self.output_end - self.position);
        buf[..count].copy_from_slice(&self.output[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::{move_mtf, move_mtf_bytes, move_mtf_forward, move_mtf_paired, move_mtf_reverse};
    #[test]
    fn byte_mtf_preserves_evolving_reduced_alphabets_and_unused_tail() {
        for size in [
            1, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256,
        ] {
            let mut expected = [0xa5u8; 256];
            for (i, value) in expected[..size].iter_mut().enumerate() {
                *value = (i as u8).wrapping_mul(73);
            }
            let mut actual = expected;
            for index in (0..size).chain((0..size).rev()).cycle().take(size * 8) {
                let byte = actual[index];
                move_mtf_bytes(&mut actual, index, byte);
                expected[..=index].rotate_right(1);
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn packed_mtf_matches_the_bytewise_oracle_at_every_position_and_update() {
        let initial: Vec<u8> = (0..=255).collect();
        for first in 0..256 {
            let mut expected = initial.clone();
            let mut words = [0u64; 32];
            for (index, &byte) in expected.iter().enumerate() {
                words[index / 8] |= u64::from(byte) << ((index & 7) * 8);
            }
            let mut random = 0x1234_5678u32;
            for turn in 0..33 {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                let index = if turn == 0 {
                    first
                } else {
                    random as usize & 255
                };
                let next = words[index / 8];
                let byte = (next >> ((index & 7) * 8)) as u8;
                assert_eq!(byte, expected[index]);
                let mut paired = words;
                let mut reversed = words;
                let mut forward = words;
                let mut bytes: [u8; 256] = expected.as_slice().try_into().unwrap();
                move_mtf_paired(&mut paired, index, next, byte);
                move_mtf_reverse(&mut reversed, index, next, byte);
                move_mtf_forward(&mut forward, index, next, byte);
                assert_eq!(forward, paired, "forward MTF state at index {index}");
                assert_eq!(paired, reversed, "MTF state at index {index}");
                move_mtf(&mut words, index, next, byte);
                assert_eq!(words, paired, "selected MTF state at index {index}");
                expected[..=index].rotate_right(1);
                move_mtf_bytes(&mut bytes, index, byte);
                assert_eq!(bytes.as_slice(), expected);
                for (index, &byte) in expected.iter().enumerate() {
                    assert_eq!((words[index / 8] >> ((index & 7) * 8)) as u8, byte);
                }
            }
        }
    }
}

#[cfg(test)]
mod cold_pending_tests {
    use super::OutputState as BlockOutput;

    fn chain(raw: &[u8], plain: &[u8], randomized: bool) -> BlockOutput {
        let links: Vec<u32> = raw
            .iter()
            .enumerate()
            .map(|(i, &byte)| ((((i + 1) % raw.len()) as u32) << 8) | u32::from(byte))
            .collect();
        BlockOutput {
            position: links[0],
            links: links.into(),
            remaining: raw.len(),
            previous: 0,
            repeated: 0,
            pending: 0,
            random_index: 1,
            random_count: super::RANDOM_NUMBERS[0] - 2,
            randomized,
            crc: !0,
            expected_crc: !super::super::update_crc(!0, plain),
            delivered: 0,
        }
    }

    #[test]
    fn cold_pending_runs_preserve_every_output_edge_and_randomized_counts() {
        let raw = [
            b'A', b'A', b'A', b'A', 3, b'B', b'C', b'C', b'C', b'C', 0, b'D', b'D', b'D', b'D',
            255, b'E',
        ];
        let mut plain = vec![b'A'; 7];
        plain.push(b'B');
        plain.extend([b'C'; 4]);
        plain.extend([b'D'; 259]);
        plain.push(b'E');
        let prefix: Vec<u8> = (0..613).map(|i| (i & 1) as u8).collect();
        let mut randomized_raw = prefix.clone();
        randomized_raw.extend([b'Q', b'Q', b'Q', b'Q', 3]);
        let tail: Vec<u8> = (0..800).map(|i| 100 + (i % 73) as u8).collect();
        randomized_raw.extend_from_slice(&tail);
        let mut randomized_plain = prefix;
        randomized_plain.extend([b'Q'; 7]);
        randomized_plain.extend(tail);
        // Published legacy randomization toggles these raw-symbol positions;
        // the first one is an RLE count, rather than a delivered byte.
        randomized_raw[617] ^= 1;
        randomized_raw[1337] ^= 1;
        let cases = [
            (raw.to_vec(), plain, false),
            (vec![b'Q'; 4], vec![b'Q'; 4], false),
            (randomized_raw, randomized_plain, true),
        ];
        for (raw, plain, randomized) in cases {
            for sizes in [&[1usize][..], &[0, 1, 0, 2, 3, 4, 255, 256, 0], &[1024]] {
                let mut state = chain(&raw, &plain, randomized);
                let mut decoded = Vec::new();
                let mut turn = 0;
                while !state.finished() {
                    let size = sizes[turn % sizes.len()];
                    turn += 1;
                    let before = (
                        state.position,
                        state.remaining,
                        state.previous,
                        state.repeated,
                        state.pending,
                        state.random_index,
                        state.random_count,
                        state.crc,
                    );
                    let mut bytes = vec![0; size];
                    let n = state.read(&mut bytes).unwrap();
                    decoded.extend_from_slice(&bytes[..n]);
                    if size == 0 {
                        assert_eq!(n, 0);
                        assert_eq!(
                            before,
                            (
                                state.position,
                                state.remaining,
                                state.previous,
                                state.repeated,
                                state.pending,
                                state.random_index,
                                state.random_count,
                                state.crc
                            )
                        );
                    }
                }
                assert_eq!(decoded, plain);
                assert_eq!(state.read(&mut []).unwrap(), 0);
            }
            let mut failed = chain(&raw, &plain, randomized);
            failed.expected_crc ^= 1;
            assert!(failed.read(&mut vec![0; plain.len() + 1]).is_err());
            assert!(failed.read(&mut []).is_err());
        }
    }
}

#[cfg(test)]
mod paired_state_tests {
    type BlockOutput = super::OutputState<[u32; 2]>;

    fn chain(raw: &[u8], plain: &[u8], randomized: bool) -> BlockOutput {
        let links: Vec<u32> = raw
            .iter()
            .enumerate()
            .map(|(i, &byte)| ((((i + 1) % raw.len()) as u32) << 8) | u32::from(byte))
            .collect();
        let paired: Vec<[u32; 2]> = links
            .iter()
            .map(|&first| [first, links[(first >> 8) as usize]])
            .collect();
        BlockOutput {
            position: links[0],
            links: paired.into(),
            remaining: raw.len(),
            previous: 0,
            repeated: 0,
            pending: 0,
            random_index: 1,
            random_count: super::RANDOM_NUMBERS[0] - 2,
            randomized,
            crc: !0,
            expected_crc: !super::super::update_crc(!0, plain),
            delivered: 0,
        }
    }

    #[test]
    fn paired_walk_matches_single_walk_at_every_output_edge_and_state() {
        let mut cases = vec![
            b"a".to_vec(),
            b"ab".to_vec(),
            b"abc".to_vec(),
            b"abcdabcdabcdabcd".to_vec(),
            vec![b'A', b'A', b'A', b'A', 0, b'B'],
            vec![b'A', b'A', b'A', b'A', 255, b'B', b'B', b'B', b'B', 3],
            vec![b'Q'; 4],
        ];
        let long: Vec<_> = (0..65_539).map(|i| (i % 251) as u8).collect();
        cases.push(long.clone());
        let mut mixed = long[..65_530].to_vec();
        mixed.extend([b'Q', b'Q', b'Q', b'Q', 255, b'Z', b'Y']);
        cases.push(mixed);
        let widths: Vec<_> = (1..=40)
            .chain([255, 256, 257, 65_535, 65_536, 65_537])
            .collect();
        for raw in &cases {
            for randomized in [false, true] {
                let sizes = if raw.len() > 1000 {
                    &widths[40..]
                } else {
                    &widths[..]
                };
                for &size in sizes {
                    let mut baseline = chain(raw, &[], randomized);
                    let mut candidate = chain(raw, &[], randomized);
                    // Force a randomization boundary inside these short cases.
                    baseline.random_count = 2;
                    candidate.random_count = 2;
                    loop {
                        for width in [0, size] {
                            let mut expected = vec![0xa5; size + 3];
                            let mut actual = expected.clone();
                            let result = if randomized {
                                baseline.read_single::<true>(&mut expected[..width])
                            } else {
                                baseline.read_single::<false>(&mut expected[..width])
                            };
                            let got = candidate.read(&mut actual[..width]);
                            assert_eq!(got.is_err(), result.is_err());
                            assert_eq!(got.ok(), result.ok());
                            assert_eq!(actual, expected);
                            assert_eq!(
                                (
                                    candidate.position,
                                    candidate.remaining,
                                    candidate.previous,
                                    candidate.repeated,
                                    candidate.pending,
                                    candidate.random_count,
                                    candidate.random_index,
                                    candidate.crc
                                ),
                                (
                                    baseline.position,
                                    baseline.remaining,
                                    baseline.previous,
                                    baseline.repeated,
                                    baseline.pending,
                                    baseline.random_count,
                                    baseline.random_index,
                                    baseline.crc
                                ),
                            );
                        }
                        if baseline.finished() {
                            assert!(candidate.finished());
                            break;
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cold_pending_runs_preserve_every_output_edge_and_randomized_counts() {
        let raw = [
            b'A', b'A', b'A', b'A', 3, b'B', b'C', b'C', b'C', b'C', 0, b'D', b'D', b'D', b'D',
            255, b'E',
        ];
        let mut plain = vec![b'A'; 7];
        plain.push(b'B');
        plain.extend([b'C'; 4]);
        plain.extend([b'D'; 259]);
        plain.push(b'E');
        let prefix: Vec<u8> = (0..613).map(|i| (i & 1) as u8).collect();
        let mut randomized_raw = prefix.clone();
        randomized_raw.extend([b'Q', b'Q', b'Q', b'Q', 3]);
        let tail: Vec<u8> = (0..800).map(|i| 100 + (i % 73) as u8).collect();
        randomized_raw.extend_from_slice(&tail);
        let mut randomized_plain = prefix;
        randomized_plain.extend([b'Q'; 7]);
        randomized_plain.extend(tail);
        // Published legacy randomization toggles these raw-symbol positions;
        // the first one is an RLE count, rather than a delivered byte.
        randomized_raw[617] ^= 1;
        randomized_raw[1337] ^= 1;
        let cases = [
            (raw.to_vec(), plain, false),
            (vec![b'Q'; 4], vec![b'Q'; 4], false),
            (randomized_raw, randomized_plain, true),
        ];
        for (raw, plain, randomized) in cases {
            for sizes in [&[1usize][..], &[0, 1, 0, 2, 3, 4, 255, 256, 0], &[1024]] {
                let mut state = chain(&raw, &plain, randomized);
                let mut decoded = Vec::new();
                let mut turn = 0;
                while !state.finished() {
                    let size = sizes[turn % sizes.len()];
                    turn += 1;
                    let before = (
                        state.position,
                        state.remaining,
                        state.previous,
                        state.repeated,
                        state.pending,
                        state.random_index,
                        state.random_count,
                        state.crc,
                    );
                    let mut bytes = vec![0; size];
                    let n = state.read(&mut bytes).unwrap();
                    decoded.extend_from_slice(&bytes[..n]);
                    if size == 0 {
                        assert_eq!(n, 0);
                        assert_eq!(
                            before,
                            (
                                state.position,
                                state.remaining,
                                state.previous,
                                state.repeated,
                                state.pending,
                                state.random_index,
                                state.random_count,
                                state.crc
                            )
                        );
                    }
                }
                assert_eq!(decoded, plain);
                assert_eq!(state.read(&mut []).unwrap(), 0);
            }
            let mut failed = chain(&raw, &plain, randomized);
            failed.expected_crc ^= 1;
            assert!(failed.read(&mut vec![0; plain.len() + 1]).is_err());
            assert!(failed.read(&mut []).is_err());
        }
    }
}

mod incremental;
pub(crate) use incremental::Incremental;

#[cfg(test)]
mod selector_storage_tests {
    use super::Parser;

    fn prefix(tables: u32, selectors: u32, invalid_tail: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut position = 0;
        let mut put = |value: u32, width: u32| {
            for shift in (0..width).rev() {
                if position % 8 == 0 {
                    bytes.push(0);
                }
                *bytes.last_mut().unwrap() |= ((value >> shift) as u8 & 1) << (7 - position % 8);
                position += 1;
            }
        };
        for byte in b"BZh9" {
            put(u32::from(*byte), 8);
        }
        put(0x314159, 24);
        put(0x265359, 24);
        put(0, 32);
        put(0, 1);
        put(0, 24);
        put(1, 16);
        put(1, 16);
        put(tables, 3);
        put(selectors, 15);
        for index in 0..selectors {
            let rank = if invalid_tail && index + 1 == selectors {
                tables
            } else {
                index % tables
            };
            for _ in 0..rank {
                put(1, 1);
            }
            put(0, 1);
        }
        // Reaching this deliberately invalid first table proves all declared
        // selectors were consumed, including those beyond the storage cap.
        put(0, 5);
        bytes
    }

    #[test]
    fn byte_selectors_preserve_validation_after_the_storage_cap() {
        for tables in 2..=6 {
            for selectors in [1, 18002, 18003, 32767] {
                for invalid_tail in [false, true] {
                    let input = prefix(tables, selectors, invalid_tail);
                    let error = Parser::new(input.as_slice()).next().err().unwrap();
                    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
                    assert_eq!(
                        error.to_string(),
                        if invalid_tail {
                            "invalid BZip2 selector"
                        } else {
                            "invalid Huffman length"
                        }
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod output_storage_tests {
    use super::Decoder;
    use std::io::{Read, Write};

    #[test]
    fn initialized_output_storage_exposes_only_successful_bytes() {
        let plain: Vec<u8> = (0..310_003).map(|i| (i * 173 + i / 17) as u8).collect();
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(1));
        encoder.write_all(&plain).unwrap();
        let encoded = encoder.finish().unwrap();
        for threads in [1, 2] {
            let mut decoder = Decoder::with_low_memory(encoded.as_slice(), threads);
            assert_eq!(decoder.read(&mut []).unwrap(), 0);
            assert!(decoder.output.is_empty());
            let mut actual = Vec::new();
            let mut buf = [0xa5; 7919];
            loop {
                assert_eq!(decoder.read(&mut []).unwrap(), 0);
                let n = decoder.read(&mut buf).unwrap();
                assert!(decoder.position <= decoder.output_end);
                assert!(decoder.output_end <= decoder.output.len());
                if n == 0 {
                    break;
                }
                if threads == 1 {
                    assert_eq!(decoder.output.len(), 1 << 20);
                }
                actual.extend_from_slice(&buf[..n]);
            }
            assert_eq!(actual, plain);
            for _ in 0..4 {
                assert_eq!(decoder.read(&mut buf).unwrap(), 0);
                assert_eq!(decoder.read(&mut []).unwrap(), 0);
            }
        }
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(1));
        encoder.write_all(b"short final chunk").unwrap();
        let mut corrupt = encoder.finish().unwrap();
        corrupt[10] ^= 1;
        for threads in [1, 2] {
            let mut decoder = Decoder::with_low_memory(corrupt.as_slice(), threads);
            let mut destination = [0xa5; 17];
            assert!(decoder.read(&mut destination).is_err());
            assert_eq!(destination, [0xa5; 17]);
            assert!(decoder.read(&mut destination).is_err());
            assert_eq!(decoder.read(&mut []).unwrap(), 0);
        }
    }
}

#[cfg(test)]
mod parser_workspace_tests {
    use super::{Decoder, Huffman, Parser};
    use std::io::Read;

    fn stream(tables: u32, full: bool, selectors: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut position = 0;
        let byte = if full { 0 } else { b'A' };
        let crc = !super::super::update_crc(!0, &[byte]);
        let mut put = |value: u32, width: u32| {
            for shift in (0..width).rev() {
                if position % 8 == 0 {
                    bytes.push(0);
                }
                *bytes.last_mut().unwrap() |= ((value >> shift) as u8 & 1) << (7 - position % 8);
                position += 1;
            }
        };
        for byte in b"BZh9" {
            put(u32::from(*byte), 8);
        }
        put(0x314159, 24);
        put(0x265359, 24);
        put(crc, 32);
        put(0, 1);
        put(0, 24);
        if full {
            put(65535, 16);
            for _ in 0..16 {
                put(65535, 16);
            }
        } else {
            put(1 << 11, 16);
            put(1 << 14, 16);
        }
        put(tables, 3);
        put(selectors, 15);
        for _ in 0..selectors {
            for _ in 1..tables {
                put(1, 1);
            }
            put(0, 1);
        }
        let length = if full { 9 } else { 2 };
        let alphabet = if full { 258 } else { 3 };
        for _ in 0..tables {
            put(length, 5);
            for _ in 0..alphabet {
                put(0, 1);
            }
        }
        put(0, length); // RUNA: one copy of the first alphabet byte.
        put(alphabet - 1, length); // EOB.
        put(0x177245, 24);
        put(0x385090, 24);
        put(crc, 32);
        bytes
    }

    fn assert_table(actual: &Huffman, expected: &Huffman) {
        assert_eq!(actual.first, expected.first);
        assert_eq!(actual.counts, expected.counts);
        assert_eq!(actual.offsets, expected.offsets);
        assert_eq!(actual.symbols, expected.symbols);
        assert_eq!(actual.fast, expected.fast);
    }

    #[test]
    fn rebuilt_tables_reset_large_small_incomplete_and_failed_state() {
        let mut table = Huffman::new(&[9; 258]).unwrap();
        let pointer = table.symbols.as_ptr();
        let capacity = table.symbols.capacity();
        for lengths in [
            vec![2; 3],
            vec![20; 3],
            vec![1; 3],
            vec![0; 3],
            vec![9; 258],
            vec![2; 3],
        ] {
            let expected = Huffman::new(&lengths);
            let actual = table.rebuild(&lengths);
            assert_eq!(
                actual.as_ref().err().map(ToString::to_string),
                expected.as_ref().err().map(ToString::to_string)
            );
            if let Ok(expected) = expected {
                assert_table(&table, &expected);
            }
            assert_eq!(table.symbols.as_ptr(), pointer);
            assert_eq!(table.symbols.capacity(), capacity);
        }
    }

    #[test]
    fn concatenated_blocks_reuse_all_warmed_entropy_allocations() {
        let mut encoded = stream(6, true, 18002);
        encoded.extend(stream(2, false, 1));
        encoded.extend(stream(4, true, 18003));
        encoded.extend(stream(2, false, 3));
        let mut parser = Parser::new(encoded.as_slice());
        assert_eq!(parser.next().unwrap().unwrap().last, [0]);
        let workspace = &parser.workspace;
        let owners = (
            workspace.alphabet.as_ptr(),
            workspace.selectors.as_ptr(),
            workspace.lengths.as_ptr(),
            workspace.tables.as_ptr(),
        );
        let capacities = (
            workspace.alphabet.capacity(),
            workspace.selectors.capacity(),
            workspace.lengths.capacity(),
            workspace.tables.capacity(),
        );
        let symbols: Vec<_> = workspace
            .tables
            .iter()
            .map(|table| (table.symbols.as_ptr(), table.symbols.capacity()))
            .collect();
        for expected in [b'A', 0, b'A'] {
            assert_eq!(parser.next().unwrap().unwrap().last, [expected]);
            let workspace = &parser.workspace;
            assert_eq!(
                (
                    workspace.alphabet.as_ptr(),
                    workspace.selectors.as_ptr(),
                    workspace.lengths.as_ptr(),
                    workspace.tables.as_ptr()
                ),
                owners
            );
            assert_eq!(
                (
                    workspace.alphabet.capacity(),
                    workspace.selectors.capacity(),
                    workspace.lengths.capacity(),
                    workspace.tables.capacity()
                ),
                capacities
            );
            assert_eq!(
                workspace
                    .tables
                    .iter()
                    .map(|table| (table.symbols.as_ptr(), table.symbols.capacity()))
                    .collect::<Vec<_>>(),
                symbols
            );
        }
        assert!(parser.next().unwrap().is_none());
        for threads in [1, 2] {
            let mut decoder = Decoder::with_low_memory(encoded.as_slice(), threads);
            let mut output = Vec::new();
            decoder.read_to_end(&mut output).unwrap();
            assert_eq!(output, [0, b'A', 0, b'A']);
        }
    }
}
