//! Checked pre-slot encoder retained as a differential oracle.
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
};
use std::thread::{self, JoinHandle};

use super::{EncoderOptions, block_crc, find_and_move, repeated_prefix, sort};

#[derive(Default)]
pub(super) struct Bits {
    pub(super) bytes: Vec<u8>,
    pub(super) pending: u8,
    pub(super) count: u8,
}

impl Bits {
    pub(super) fn len(&self) -> usize {
        self.bytes.len() * 8 + usize::from(self.count)
    }
    fn put(&mut self, value: u32, count: u8) {
        let combined = (u64::from(self.pending) << count) | u64::from(value);
        let mut remaining = self.count + count;
        while remaining >= 8 {
            remaining -= 8;
            self.bytes.push((combined >> remaining) as u8);
        }
        self.pending = (combined & ((1u64 << remaining) - 1)) as u8;
        self.count = remaining;
    }
    fn append(&mut self, other: &Self) {
        if self.count == 0 {
            self.bytes.extend_from_slice(&other.bytes);
        } else {
            // Full source bytes leave the destination alignment unchanged.
            // Keep that alignment out of the loop instead of invoking the
            // variable-width writer for every byte of every candidate block.
            // Include a final carry so the bulk reserve cannot be followed
            // by geometric growth solely to append one remaining byte.
            let carry = usize::from(self.count + other.count >= 8);
            self.bytes.reserve(other.bytes.len() + carry);
            let shift = self.count;
            let mask = (1u8 << shift) - 1;
            for &byte in &other.bytes {
                self.bytes
                    .push((self.pending << (8 - shift)) | (byte >> shift));
                self.pending = byte & mask;
            }
        }
        if other.count != 0 {
            self.put(u32::from(other.pending), other.count);
        }
    }
    fn drain<W: Write>(&mut self, output: &mut W) -> io::Result<()> {
        output.write_all(&self.bytes)?;
        self.bytes.clear();
        Ok(())
    }
}

struct Encoded {
    bits: Bits,
    crcs: Vec<u32>,
}

struct Worker<T>(Option<JoinHandle<T>>);

impl<T> Worker<T> {
    fn join(mut self) -> io::Result<T> {
        self.0
            .take()
            .unwrap()
            .join()
            .map_err(|_| io::Error::other("BZip2 worker panicked"))
    }
}

impl<T> Drop for Worker<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = handle.join();
        }
    }
}

struct Job {
    sequence: usize,
    input: Vec<u8>,
    passes: u32,
}

type JobResult = (usize, io::Result<Encoded>);

fn execute_job(job: Job) -> JobResult {
    let sequence = job.sequence;
    let result =
        std::panic::catch_unwind(move || encode_tree(&job.input, job.passes, job.passes > 1))
            .map_err(|_| io::Error::other("BZip2 worker panicked"));
    (sequence, result)
}

struct Pool {
    jobs: Option<SyncSender<Job>>,
    results: Option<Receiver<JobResult>>,
    input: Arc<Mutex<Receiver<Job>>>,
    workers: Vec<Worker<()>>,
    cancelled: Arc<AtomicBool>,
}

impl Pool {
    fn new(threads: usize) -> io::Result<Self> {
        let (jobs, input) = mpsc::sync_channel::<Job>(threads);
        let input = Arc::new(Mutex::new(input));
        let (output, results) = mpsc::channel();
        // Establish teardown ownership before the first fallible spawn.
        let mut pool = Self {
            jobs: Some(jobs),
            results: Some(results),
            input: input.clone(),
            workers: Vec::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        for _ in 1..threads {
            let input = input.clone();
            let output = output.clone();
            let cancelled = pool.cancelled.clone();
            let worker = thread::Builder::new().spawn(move || {
                loop {
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let job = match input.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => break,
                    };
                    let Ok(job) = job else {
                        break;
                    };
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let result = execute_job(job);
                    let failed = result.1.is_err();
                    if output.send(result).is_err() || failed {
                        break;
                    }
                }
            })?;
            pool.workers.push(Worker(Some(worker)));
        }
        Ok(pool)
    }

    fn take_job(&self) -> Option<Job> {
        self.input.try_lock().ok()?.try_recv().ok()
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        drop(self.jobs.take());
        drop(self.results.take());
        {
            let input = self
                .input
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            while input.try_recv().is_ok() {}
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// An incremental BZip2 writer. `flush` preserves the current block.
pub struct Encoder<W: Write> {
    output: W,
    options: EncoderOptions,
    bits: Bits,
    block: Vec<u8>,
    combined_crc: u32,
    run_byte: u8,
    run_count: usize,
    block_limit: usize,
    pending: VecDeque<Option<Encoded>>,
    output_sequence: usize,
    pool: Option<Pool>,
    failure: bool,
}

impl<W: Write> Encoder<W> {
    pub fn new(output: W, mut options: EncoderOptions) -> Self {
        options.level = options.level.min(9);
        options.threads = options.threads.clamp(1, 64);
        #[cfg(target_arch = "wasm32")]
        {
            options.threads = 1;
        }
        let multiplier = options
            .block_size
            .map(|size| (size / 100_000).clamp(1, 9) as usize)
            .unwrap_or_else(|| match options.level {
                0 | 1 => 1,
                2 => 3,
                3 => 5,
                4 => 7,
                _ => 9,
            });
        options.passes = Some(
            options
                .passes
                .unwrap_or(match options.level {
                    7 | 8 => 2,
                    _ => 1,
                })
                .clamp(1, 10),
        );
        let mut bits = Bits::default();
        for byte in [b'B', b'Z', b'h', b'0' + multiplier as u8] {
            bits.put(u32::from(byte), 8);
        }
        Self {
            output,
            options,
            bits,
            block: Vec::new(),
            combined_crc: 0,
            run_byte: 0,
            run_count: 0,
            block_limit: multiplier * 100_000 - 1,
            pending: VecDeque::new(),
            output_sequence: 0,
            pool: None,
            failure: false,
        }
    }

    pub fn get_ref(&self) -> &W {
        &self.output
    }
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.output
    }

    fn accept(&mut self, block: Encoded) -> io::Result<()> {
        for crc in block.crcs {
            self.combined_crc = self.combined_crc.rotate_left(1) ^ crc;
        }
        self.bits.append(&block.bits);
        self.bits.drain(&mut self.output)
    }

    fn wait_first(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        while self.pending.front().is_some_and(Option::is_none) {
            if let Some(job) = self.pool.as_ref().unwrap().take_job() {
                self.store_result(execute_job(job))?;
                continue;
            }
            let result = self
                .pool
                .as_ref()
                .unwrap()
                .results
                .as_ref()
                .unwrap()
                .recv()
                .map_err(|_| io::Error::other("BZip2 worker result channel closed"))?;
            self.store_result(result)?;
        }
        let encoded = self.pending.pop_front().unwrap().unwrap();
        self.output_sequence += 1;
        self.accept(encoded)
    }

    fn store_result(&mut self, (sequence, result): JobResult) -> io::Result<()> {
        let offset = sequence
            .checked_sub(self.output_sequence)
            .ok_or_else(|| io::Error::other("BZip2 worker result preceded output"))?;
        let slot = self
            .pending
            .get_mut(offset)
            .ok_or_else(|| io::Error::other("BZip2 worker result exceeded pending blocks"))?;
        if slot.is_some() {
            return Err(io::Error::other("duplicate BZip2 worker result"));
        }
        *slot = Some(result?);
        Ok(())
    }

    fn harvest_completed(&mut self) -> io::Result<()> {
        if self.pool.is_some() {
            loop {
                let result = self
                    .pool
                    .as_ref()
                    .unwrap()
                    .results
                    .as_ref()
                    .unwrap()
                    .try_recv();
                match result {
                    Ok(result) => self.store_result(result)?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if self.pending.iter().any(Option::is_none) {
                            return Err(io::Error::other("BZip2 worker result channel closed"));
                        }
                        break;
                    }
                }
            }
        }
        while self.pending.front().is_some_and(Option::is_some) {
            self.wait_first()?;
        }
        Ok(())
    }

    fn submit(&mut self) -> io::Result<()> {
        if self.block.is_empty() {
            return Ok(());
        }
        let input = std::mem::take(&mut self.block);
        let passes = self.options.passes.unwrap();
        if self.options.threads == 1 {
            self.accept(encode_tree(&input, passes, passes > 1))?;
        } else {
            if self.pool.is_none() {
                self.pool = Some(Pool::new(self.options.threads)?);
            }
            // Keep jobs prepared ahead of the workers. While the caller helps
            // with a full queue, workers keep taking the already-prepared jobs.
            // Ordered slots bound active jobs, queued input and ready results.
            self.harvest_completed()?;
            if self.pending.len() >= self.options.threads * 2 {
                self.wait_first()?;
                self.harvest_completed()?;
            }
            let sequence = self
                .output_sequence
                .checked_add(self.pending.len())
                .ok_or_else(|| io::Error::other("BZip2 block sequence overflow"))?;
            self.pending.push_back(None);
            let job = Job {
                sequence,
                input,
                passes,
            };
            match self
                .pool
                .as_ref()
                .unwrap()
                .jobs
                .as_ref()
                .unwrap()
                .try_send(job)
            {
                Ok(()) => {}
                Err(TrySendError::Full(job)) => {
                    self.store_result(execute_job(job))?;
                    self.harvest_completed()?;
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(io::Error::other("BZip2 worker job channel closed"));
                }
            }
        }
        Ok(())
    }

    fn emit_run(&mut self) -> io::Result<()> {
        if self.run_count >= 4 {
            self.block.push((self.run_count - 4) as u8);
        }
        self.run_count = 0;
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<W> {
        if self.failure {
            return Err(io::Error::other("BZip2 encoder is in a failed state"));
        }
        self.emit_run()?;
        self.submit()?;
        while !self.pending.is_empty() {
            self.wait_first()?;
        }
        self.bits.put(0x177245, 24);
        self.bits.put(0x385090, 24);
        self.bits.put(self.combined_crc, 32);
        if self.bits.count != 0 {
            self.bits.put(0, 8 - self.bits.count);
        }
        self.bits.drain(&mut self.output)?;
        self.output.flush()?;
        Ok(self.output)
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if self.failure {
            return Err(io::Error::other("BZip2 encoder is in a failed state"));
        }
        let result = (|| {
            let mut position = 0;
            while position < input.len() {
                if self.block.len() >= self.block_limit {
                    self.emit_run()?;
                    self.submit()?;
                }
                // After four equal bytes RLE1 emits nothing until the run
                // ends or reaches 259. Count that region a word at a time;
                // the block boundary cannot change while no bytes are emitted.
                if self.run_count >= 4 && input[position] == self.run_byte {
                    let limit = (259 - self.run_count).min(input.len() - position);
                    let count = repeated_prefix(&input[position..position + limit], self.run_byte);
                    position += count;
                    self.run_count += count;
                    if self.run_count == 259 {
                        self.block.push(255);
                        self.run_count = 0;
                    }
                    continue;
                }
                let byte = input[position];
                position += 1;
                if self.run_count == 0 || byte != self.run_byte {
                    self.emit_run()?;
                    self.block.push(byte);
                    self.run_byte = byte;
                    self.run_count = 1;
                } else {
                    self.run_count += 1;
                    if self.run_count <= 4 {
                        self.block.push(byte);
                    } else if self.run_count == 259 {
                        self.block.push(255);
                        self.run_count = 0;
                    }
                }
            }
            Ok(input.len())
        })();
        if result.is_err() {
            self.failure = true;
            drop(self.pool.take());
            self.pending.clear();
        }
        result
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.failure {
            return Err(io::Error::other("BZip2 encoder is in a failed state"));
        }
        let result = (|| {
            while !self.pending.is_empty() {
                self.wait_first()?;
            }
            self.bits.drain(&mut self.output)?;
            self.output.flush()
        })();
        if result.is_err() {
            self.failure = true;
            drop(self.pool.take());
            self.pending.clear();
        }
        result
    }
}

pub(super) fn symbols(input: &[u8], used: &[u8]) -> Vec<u16> {
    let mut alphabet = [0u64; 32];
    for (index, &byte) in used.iter().enumerate() {
        alphabet[index / 8] |= u64::from(byte) << ((index & 7) * 8);
    }
    let mut result = Vec::with_capacity(input.len() + 1);
    let mut zeros = 0usize;
    let flush_zeros = |result: &mut Vec<u16>, mut zeros: usize| {
        while zeros != 0 {
            zeros -= 1;
            result.push((zeros & 1) as u16);
            zeros >>= 1;
        }
    };
    for &byte in input {
        if alphabet[0] as u8 == byte {
            zeros += 1;
        } else {
            flush_zeros(&mut result, zeros);
            zeros = 0;
            let index = find_and_move(&mut alphabet, byte);
            result.push((index + 1) as u16);
        }
    }
    flush_zeros(&mut result, zeros);
    result.push((used.len() + 1) as u16);
    result
}

// EncodeBlock's MTF search and move, processing eight positions per word.
// The least zero byte of (word XOR repeated-byte) identifies the first match;
// borrow propagation can only mark higher bytes after that first match.
// Huffman_Generate, public-domain C/HuffEnc.c at the reference revision above.
// Sorted leaves and generated nodes form two monotone queues.
pub(super) fn lengths(frequencies: &[u32]) -> Vec<u8> {
    let n = frequencies.len();
    let mut parents = vec![usize::MAX; 2 * n - 1];
    let mut weights = vec![0u32; 2 * n - 1];
    let mut leaves: Vec<_> = frequencies
        .iter()
        .enumerate()
        .map(|(i, &f)| (f.max(1), i))
        .collect();
    leaves.sort_unstable();
    let mut leaf = 0;
    let mut internal = n;
    for node in n..2 * n - 1 {
        for _ in 0..2 {
            let (weight, child) =
                if internal == node || (leaf < n && leaves[leaf].0 <= weights[internal]) {
                    let result = leaves[leaf];
                    leaf += 1;
                    result
                } else {
                    let result = (weights[internal], internal);
                    internal += 1;
                    result
                };
            parents[child] = node;
            weights[node] += weight;
        }
    }
    // Huffman_Generate: visit internal nodes from root toward their children,
    // splitting the nearest available leaf when the depth limit is reached.
    let mut population = [0usize; 17];
    population[1] = 2;
    for i in (n..2 * n - 2).rev() {
        let mut depth = 0;
        let mut node = i;
        while parents[node] != usize::MAX {
            node = parents[node];
            depth += 1;
        }
        if depth >= 16 {
            depth = (1..16).rev().find(|&i| population[i] != 0).unwrap();
        }
        population[depth] -= 1;
        population[depth + 1] += 2;
    }
    let mut result = vec![0u8; n];
    let mut leaf = 0;
    for depth in (1..=16).rev() {
        for _ in 0..population[depth] {
            result[leaves[leaf].1] = depth as u8;
            leaf += 1;
        }
    }
    result
}

pub(super) fn codes(lengths: &[u8]) -> Vec<u32> {
    let mut result = vec![0u32; lengths.len()];
    let mut code = 0;
    for length in 1..=20 {
        for (symbol, &size) in lengths.iter().enumerate() {
            if size == length {
                result[symbol] = code;
                code += 1;
            }
        }
        code <<= 1;
    }
    result
}

pub(super) fn tables(
    symbols: &[u16],
    alphabet: usize,
    groups: usize,
) -> (Vec<Vec<u8>>, Vec<usize>) {
    let mut frequency = vec![0u32; alphabet];
    for &symbol in symbols {
        frequency[usize::from(symbol)] += 1;
    }
    let mut tables = vec![vec![1u8; alphabet]; groups];
    let mut remaining = symbols.len();
    let mut start = 0;
    for partition in 0..groups {
        let target = remaining / (groups - partition);
        let mut end = start;
        let mut sum = 0;
        while end < alphabet && sum < target {
            sum += frequency[end] as usize;
            end += 1;
        }
        if end > start + 1 && partition != 0 && partition + 1 != groups && partition & 1 != 0 {
            end -= 1;
            sum -= frequency[end] as usize;
        }
        tables[groups - partition - 1][start..end].fill(0);
        remaining -= sum;
        start = end;
    }
    let mut selectors = Vec::new();
    for _ in 0..4 {
        let mut frequencies = vec![vec![0u32; alphabet]; groups];
        selectors.clear();
        for chunk in symbols.chunks(50) {
            let best = (0..groups)
                .min_by_key(|&table| {
                    chunk
                        .iter()
                        .map(|&s| u32::from(tables[table][usize::from(s)]))
                        .sum::<u32>()
                })
                .unwrap();
            selectors.push(best);
            for &symbol in chunk {
                frequencies[best][usize::from(symbol)] += 1;
            }
        }
        for (table, frequency) in tables.iter_mut().zip(frequencies) {
            *table = lengths(&frequency);
        }
    }
    (tables, selectors)
}

fn encode_block(input: &[u8], optimize: bool) -> Encoded {
    let crc = block_crc(input);
    let (transformed, origin) = sort::transform(input);
    let mut present = [false; 256];
    for &byte in input {
        present[usize::from(byte)] = true;
    }
    let used: Vec<u8> = (0..=255).filter(|&i| present[i]).map(|i| i as u8).collect();
    let symbols = symbols(&transformed, &used);
    let group_count = match symbols.len() {
        0..200 => 2,
        200..600 => 3,
        600..1200 => 4,
        1200..2400 => 5,
        _ => 6,
    };

    let mut bits = Bits::default();
    bits.put(0x314159, 24);
    bits.put(0x265359, 24);
    bits.put(crc, 32);
    bits.put(0, 1);
    bits.put(origin as u32, 24);
    for group in present.chunks(16) {
        bits.put(u32::from(group.iter().any(|&v| v)), 1);
    }
    for group in present.chunks(16) {
        if group.iter().any(|&v| v) {
            for &value in group {
                bits.put(u32::from(value), 1);
            }
        }
    }
    let candidates = if optimize {
        2..=6
    } else {
        group_count..=group_count
    };
    let mut best: Option<Bits> = None;
    for count in candidates {
        let candidate = encode_symbols(&symbols, used.len() + 2, count);
        if best
            .as_ref()
            .is_none_or(|best| candidate.len() <= best.len())
        {
            best = Some(candidate);
        }
    }
    bits.append(&best.unwrap());
    Encoded {
        bits,
        crcs: vec![crc],
    }
}

pub(super) fn encode_symbols(symbols: &[u16], alphabet: usize, group_count: usize) -> Bits {
    let (tables, selectors) = tables(symbols, alphabet, group_count);
    let codes: Vec<_> = tables.iter().map(|lengths| codes(lengths)).collect();
    let mut bits = Bits::default();
    bits.put(group_count as u32, 3);
    bits.put(selectors.len() as u32, 15);
    let mut order: Vec<_> = (0..group_count).collect();
    for &selector in &selectors {
        let index = order.iter().position(|&table| table == selector).unwrap();
        for _ in 0..index {
            bits.put(1, 1);
        }
        bits.put(0, 1);
        order[..=index].rotate_right(1);
    }
    for table in &tables {
        let mut previous = table[0];
        bits.put(u32::from(previous), 5);
        for &length in table {
            while previous < length {
                bits.put(2, 2);
                previous += 1;
            }
            while previous > length {
                bits.put(3, 2);
                previous -= 1;
            }
            bits.put(0, 1);
        }
    }
    for (chunk, &table) in symbols.chunks(50).zip(&selectors) {
        for &symbol in chunk {
            let s = usize::from(symbol);
            bits.put(codes[table][s], tables[table][s]);
        }
    }
    bits
}

// EncodeBlockWithHeaders: checksum the original bytes represented by RLE1.
// EncodeBlock2: compare the complete block with recursively encoded halves.
fn encode_tree(input: &[u8], passes: u32, optimize: bool) -> Encoded {
    let mut split = input.len() / 2;
    if passes > 1 && input.len() >= 1024 {
        while split < input.len()
            && (input[split] == input[split - 1] || input[split - 1] == input[split - 2])
        {
            split += 1;
        }
        if split < input.len() {
            let mut divided = encode_tree(&input[..split], passes - 1, optimize);
            let right = encode_tree(&input[split..], passes - 1, optimize);
            divided.bits.append(&right.bits);
            divided.crcs.extend(right.crcs);
            let whole = encode_block(input, optimize);
            return if whole.bits.len() < divided.bits.len() {
                whole
            } else {
                divided
            };
        }
    }
    encode_block(input, optimize)
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::{Bits, Encoded, Encoder, EncoderOptions, Worker, lengths};

    fn unpack(bits: &Bits) -> Vec<bool> {
        let mut result = Vec::new();
        for &byte in &bits.bytes {
            for bit in (0..8).rev() {
                result.push((byte >> bit) & 1 != 0);
            }
        }
        for bit in (0..bits.count).rev() {
            result.push((bits.pending >> bit) & 1 != 0);
        }
        result
    }

    #[test]
    fn appending_a_final_carry_does_not_double_the_payload_capacity() {
        for alignment in 1..8 {
            let mut destination = Bits::default();
            destination.put(0xA5, 8);
            destination.put(0, alignment);
            let source = Bits {
                bytes: vec![0x96; 4096],
                pending: 1,
                count: 8 - alignment,
            };
            let expected = [unpack(&destination), unpack(&source)].concat();
            destination.append(&source);
            assert_eq!(unpack(&destination), expected);
            assert!(
                destination.bytes.capacity() < source.bytes.len() * 2,
                "one carry byte doubled capacity at alignment {alignment}"
            );
        }
    }

    #[test]
    fn word_bit_writer_and_append_match_the_bitwise_oracle() {
        let mut state = 0x12345678u32;
        for alignment in 0..8 {
            let mut bits = Bits::default();
            let mut expected = Vec::new();
            bits.put(0, alignment);
            expected.resize(usize::from(alignment), false);
            for width in (0..=32).cycle().take(400) {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let value = if width == 32 {
                    state
                } else {
                    state & ((1u32 << width) - 1)
                };
                bits.put(value, width);
                expected.extend((0..width).rev().map(|bit| (value >> bit) & 1 != 0));
                assert_eq!(unpack(&bits), expected);
            }
            for source_alignment in 0..8 {
                let mut source = Bits::default();
                let mut source_expected = Vec::new();
                for byte in 0..=255 {
                    source.put(byte, 8);
                    source_expected.extend((0..8).rev().map(|bit| (byte >> bit) & 1 != 0));
                }
                source.put(0x55 & ((1 << source_alignment) - 1), source_alignment);
                source_expected.extend(
                    (0..source_alignment)
                        .rev()
                        .map(|bit| (0x55 >> bit) & 1 != 0),
                );
                let before = unpack(&bits);
                bits.append(&source);
                assert_eq!(unpack(&bits), [before, source_expected].concat());
                let before = unpack(&bits);
                bits.append(&Bits::default());
                assert_eq!(unpack(&bits), before);
            }
        }
    }

    #[test]
    fn thread_budget_includes_the_calling_thread() {
        for budget in [1, 2, 4] {
            let mut encoder = Encoder::new(
                Vec::new(),
                EncoderOptions {
                    threads: budget,
                    passes: Some(1),
                    ..Default::default()
                },
            );
            #[cfg(target_arch = "wasm32")]
            assert_eq!(encoder.options.threads, 1);
            for _ in 0..9 {
                encoder.block.extend_from_slice(b"small pending block");
                encoder.submit().unwrap();
                assert_eq!(
                    encoder.pool.as_ref().map_or(0, |pool| pool.workers.len()),
                    encoder.options.threads - 1
                );
                assert!(encoder.pending.len() <= encoder.options.threads * 2);
            }
            encoder.finish().unwrap();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn persistent_workers_are_reused_across_flush() {
        let mut encoder = Encoder::new(
            Vec::new(),
            EncoderOptions {
                threads: 4,
                passes: Some(1),
                ..Default::default()
            },
        );
        let mut previous = Vec::new();
        for _ in 0..8 {
            encoder.block.extend_from_slice(b"one complete block");
            encoder.submit().unwrap();
            std::io::Write::flush(&mut encoder).unwrap();
            let identities: Vec<_> = encoder
                .pool
                .as_ref()
                .unwrap()
                .workers
                .iter()
                .map(|worker| worker.0.as_ref().unwrap().thread().id())
                .collect();
            if !previous.is_empty() {
                assert_eq!(identities, previous);
            }
            previous = identities;
            assert!(encoder.pending.is_empty());
        }
        encoder.finish().unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn worker_panic_reports_its_sequence_and_teardown_joins() {
        let pool = super::Pool::new(2).unwrap();
        // Empty internal blocks violate BlockSort's entry invariant, allowing
        // the worker's panic-to-result boundary to be exercised directly.
        pool.jobs
            .as_ref()
            .unwrap()
            .send(super::Job {
                sequence: 17,
                input: Vec::new(),
                passes: 1,
            })
            .unwrap();
        let (sequence, result) = pool
            .results
            .as_ref()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(sequence, 17);
        assert!(result.is_err());
        let input = pool.input.clone();
        // The failed worker has exited. Queued inputs must be discarded by
        // cancellation even when another receiver reference remains alive.
        for sequence in 18..20 {
            assert!(
                pool.jobs
                    .as_ref()
                    .unwrap()
                    .send(super::Job {
                        sequence,
                        input: vec![42],
                        passes: 1
                    })
                    .is_ok()
            );
        }
        drop(pool);
        assert!(matches!(
            input.lock().unwrap().try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn huffman_ties_and_length_limit_match_the_public_domain_oracle() {
        // Digests generated by C/HuffEnc.c's Huffman_Generate at the cited
        // source revision. Fibonacci frequencies exercise the 16-bit limit.
        let mut fibonacci = vec![1u32, 1];
        while fibonacci.len() < 24 {
            let n = fibonacci.len();
            fibonacci.push(fibonacci[n - 1] + fibonacci[n - 2]);
        }
        let mut forward = fibonacci.clone();
        forward.resize(258, 1);
        fibonacci.reverse();
        fibonacci.resize(258, 1);
        let cases = [
            (vec![1; 3], 0xeaa6d31875e4f1acu64),
            (vec![1; 4], 0x9e749290540f5425),
            (vec![1, 1, 900000], 0xeaa6d31875e4f1ac),
            (vec![1; 258], 0xcf13c5ad6f846ed1),
            (forward, 0x3b035908f63d3d72),
            (fibonacci, 0x1ad7ef35752d20fc),
        ];
        for (frequencies, expected) in cases {
            let result = lengths(&frequencies);
            let digest = result.iter().fold(0xcbf29ce484222325u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(digest, expected);
            assert!(result.iter().all(|&len| (1..=16).contains(&len)));
            assert_eq!(
                result.iter().map(|&len| 1u32 << (16 - len)).sum::<u32>(),
                1 << 16
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn abandoned_workers_are_joined() {
        let completed = Arc::new(AtomicBool::new(false));
        let signal = completed.clone();
        let handle = std::thread::spawn(move || {
            signal.store(true, Ordering::Release);
            Encoded {
                bits: Bits::default(),
                crcs: Vec::new(),
            }
        });
        drop(Worker(Some(handle)));
        assert!(completed.load(Ordering::Acquire));
    }

    #[test]
    fn packed_mtf_matches_bytewise_search_and_move_at_every_position() {
        let mut state = 0x12345678u32;
        for size in 1..=256 {
            let mut expected: Vec<_> = (0..size).map(|i| i as u8).collect();
            let mut packed = [0u64; 32];
            for (index, &byte) in expected.iter().enumerate() {
                packed[index / 8] |= u64::from(byte) << ((index & 7) * 8);
            }
            for iteration in 0..1024 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let position = if iteration < size {
                    iteration
                } else {
                    state as usize % size
                };
                let byte = expected[position];
                assert_eq!(super::find_and_move(&mut packed, byte), position);
                expected[..=position].rotate_right(1);
                for (index, &value) in expected.iter().enumerate() {
                    assert_eq!((packed[index / 8] >> ((index & 7) * 8)) as u8, value);
                }
            }
        }
    }

    fn bitwise_crc(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 != 0 {
                    (crc << 1) ^ 0x04C1_1DB7
                } else {
                    crc << 1
                };
            }
        }
        !crc
    }

    fn rle1(bytes: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        let mut position = 0;
        while position < bytes.len() {
            let byte = bytes[position];
            let mut count = 1;
            while count < 259 && position + count < bytes.len() && bytes[position + count] == byte {
                count += 1;
            }
            encoded.extend(std::iter::repeat_n(byte, count.min(4)));
            if count >= 4 {
                encoded.push((count - 4) as u8);
            }
            position += count;
        }
        encoded
    }

    #[test]
    fn block_crc_matches_raw_bitwise_oracle_across_rle_counts_and_literal_spans() {
        assert_eq!(bitwise_crc(b"123456789"), 0xFC89_1918);
        let mut cases = vec![Vec::new(), b"123456789".to_vec()];
        for byte in [0, 1, 4, 255] {
            for count in [1, 2, 3, 4, 5, 258, 259, 260, 518, 519] {
                let mut bytes: Vec<_> = (0..1024).map(|i| i as u8).collect();
                bytes.extend(std::iter::repeat_n(byte, count));
                bytes.extend((0..1024).rev().map(|i| i as u8));
                cases.push(bytes);
            }
        }
        let mut state = 0x1234_5678u32;
        for length in [1, 2, 3, 4, 7, 8, 9, 255, 256, 257, 4096, 65536] {
            cases.push(
                (0..length)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        state as u8
                    })
                    .collect(),
            );
        }
        for bytes in cases {
            assert_eq!(
                super::block_crc(&rle1(&bytes)),
                bitwise_crc(&bytes),
                "raw length {}",
                bytes.len()
            );
        }
    }

    #[test]
    fn block_crc_preserves_independent_rle_block_boundaries() {
        let mut first: Vec<_> = (0..4096).map(|i| i as u8).collect();
        first.extend(std::iter::repeat_n(0, 4));
        first.extend(std::iter::repeat_n(255, 259));
        let mut second = vec![17; 259];
        second.extend((0..8192).map(|i| i as u8));
        second.extend(std::iter::repeat_n(1, 4));
        let mut encoded = rle1(&first);
        encoded.extend(rle1(&second));
        assert_eq!(super::block_crc(&rle1(&first)), bitwise_crc(&first));
        assert_eq!(super::block_crc(&rle1(&second)), bitwise_crc(&second));
        first.extend(second);
        assert_eq!(super::block_crc(&encoded), bitwise_crc(&first));
    }
}
