//! Move-owned reusable worker slots for incremental BZip2 encoding.
// Translation of BZip2Encoder.cpp from 7-Zip, LGPL-2.1-or-later.
// https://github.com/ip7z/7zip/blob/9128b80e3a471108678e2b3ec3c985861c8d7b0f/CPP/7zip/Compress/BZip2Encoder.cpp
// Changes: bounded owned slots, thread-owned scratch, initialized typed
// storage, joined teardown and helper threads for the independent halves of a
// recursive trial.

use super::{
    EncoderOptions, block_crc, entropy::Entropy, induced_sort::Workspace, repeated_prefix,
    trial::TrialWriter,
};
use std::{
    collections::VecDeque,
    io::{self, Write},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicIsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
};

struct Scratch {
    sort: Workspace,
    entropy: Entropy,
    trial: TrialWriter,
}

impl Scratch {
    fn new() -> Self {
        Self {
            sort: Workspace::default(),
            entropy: Entropy::new(),
            trial: TrialWriter::new(),
        }
    }

    fn block(&mut self, input: &[u8], optimize: bool) -> io::Result<u32> {
        let crc = block_crc(input);
        let (order, origin) = self.sort.sort(input)?;
        self.entropy.prepare(input, order)?;
        let mut present = [false; 256];
        for &byte in input {
            present[usize::from(byte)] = true;
        }
        let bits = &mut self.trial;
        bits.put(0x314159, 24)?;
        bits.put(0x265359, 24)?;
        bits.put(crc, 32)?;
        bits.put(0, 1)?;
        bits.put(origin as u32, 24)?;
        for group in present.chunks(16) {
            bits.put(u32::from(group.iter().any(|&v| v)), 1)?;
        }
        for group in present.chunks(16) {
            if group.iter().any(|&v| v) {
                for &value in group {
                    bits.put(u32::from(value), 1)?;
                }
            }
        }
        if optimize {
            self.entropy.optimize(bits)?;
        } else {
            let groups = match self.entropy.symbol_count() {
                0..200 => 2,
                200..600 => 3,
                600..1200 => 4,
                1200..2400 => 5,
                _ => 6,
            };
            self.entropy.encode(groups, bits)?;
        }
        Ok(crc)
    }

    // EncodeBlock2: alternatives share one output region and CRC prefix.
    fn tree(
        &mut self,
        input: &[u8],
        passes: u32,
        optimize: bool,
        helpers: Option<&Helpers>,
    ) -> io::Result<()> {
        let start = self.trial.checkpoint();
        if passes > 1 && input.len() >= 1024 {
            let mut split = input.len() / 2;
            while split < input.len()
                && (input[split] == input[split - 1] || input[split - 1] == input[split - 2])
            {
                split += 1;
            }
            if split < input.len() {
                let (left, right) = input.split_at(split);
                if !self.halves(left, right, passes - 1, optimize, helpers)? {
                    self.tree(left, passes - 1, optimize, helpers)?;
                    self.tree(right, passes - 1, optimize, helpers)?;
                }
                let divided = self.trial.checkpoint();
                let whole_start = self.trial.begin_whole(start, divided)?;
                let crc = self.block(input, optimize)?;
                self.trial.select(start, divided, whole_start, crc)?;
                return Ok(());
            }
        }
        let crc = self.block(input, optimize)?;
        self.trial.commit_crc(crc)
    }

    /// Encodes the right half's subtree on a spare thread while this thread
    /// encodes the left half's, then appends the right half's selected bits
    /// and CRCs. A subtree's selected output does not depend on the bit
    /// alignment it starts at, and every choice compares only lengths, so the
    /// bytes equal the serial order's. Returns false, having written nothing,
    /// when no thread is spare.
    fn halves(
        &mut self,
        left: &[u8],
        right: &[u8],
        passes: u32,
        optimize: bool,
        helpers: Option<&Helpers>,
    ) -> io::Result<bool> {
        if right.len() < MIN_HELPED_INPUT {
            return Ok(false);
        }
        let Some(mut claim) = helpers.and_then(Helpers::claim) else {
            return Ok(false);
        };
        let helper = &mut claim.scratch[0];
        helper.trial.reset();
        let joined = thread::scope(|scope| {
            let task = thread::Builder::new()
                .name("bzip2-encode-half".into())
                .spawn_scoped(scope, || helper.tree(right, passes, optimize, helpers))
                .ok()?;
            let left = self.tree(left, passes, optimize, helpers);
            Some((left, task.join()))
        });
        let Some((left, right)) = joined else {
            return Ok(false);
        };
        left?;
        right.unwrap_or_else(|payload| resume_unwind(payload))?;
        self.trial.append(&claim.scratch[0].trial)?;
        Ok(true)
    }
}

/// Inputs below this size are never split across threads.
const MIN_HELPED_INPUT: usize = 1 << 15;

/// Threads of an encoder's budget that are not encoding a block, lent to the
/// halves of recursive trials.
struct Helpers {
    /// Whether threads may be lent. The encoder opens lending only while it
    /// drains its last pending blocks: while blocks still arrive, a thread
    /// that is idle for a moment is not spare, and lending it left the block
    /// jobs and their helpers competing for cores once the next block came.
    lending: AtomicBool,
    /// The budget minus executing block jobs and running helpers. A block job
    /// that starts while every thread helps may take it below zero briefly.
    idle: AtomicIsize,
    /// Reusable helper scratch, at most one per thread of the budget.
    spare: Mutex<Vec<Vec<Scratch>>>,
}

impl Helpers {
    fn new(threads: usize, lending: bool) -> Self {
        Self {
            lending: AtomicBool::new(lending),
            idle: AtomicIsize::new(threads as isize),
            spare: Mutex::new(Vec::new()),
        }
    }

    fn claim(&self) -> Option<Claim<'_>> {
        if !self.lending.load(Ordering::Acquire) {
            return None;
        }
        self.idle
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |idle| {
                (idle > 0).then(|| idle - 1)
            })
            .ok()?;
        let spare = self
            .spare
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop();
        let scratch = match spare {
            Some(scratch) => scratch,
            None => {
                let mut scratch = Vec::new();
                if scratch.try_reserve_exact(1).is_err() {
                    self.idle.fetch_add(1, Ordering::AcqRel);
                    return None;
                }
                scratch.push(Scratch::new());
                scratch
            }
        };
        Some(Claim {
            helpers: self,
            scratch,
        })
    }
}

/// One lent thread and its scratch; dropping it returns both.
struct Claim<'a> {
    helpers: &'a Helpers,
    scratch: Vec<Scratch>,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        // Scratch interrupted by a panic is discarded rather than reused.
        if !thread::panicking() {
            self.helpers
                .spare
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(std::mem::take(&mut self.scratch));
        }
        self.helpers.idle.fetch_add(1, Ordering::AcqRel);
    }
}

/// The encoding scratch a thread owns: each worker, the caller and every
/// helper keep one, allocated on its first block. A one-element vector gives
/// fallible allocation, and fixed Huffman arrays never move between threads.
fn owned_scratch(owned: &mut Vec<Scratch>) -> io::Result<&mut Scratch> {
    if owned.is_empty() {
        owned.try_reserve_exact(1).map_err(io::Error::other)?;
        owned.push(Scratch::new());
    }
    Ok(&mut owned[0])
}

/// Open slots per thread of the budget. Slots hold only input and encoded
/// output, so several per thread are cheap; they let workers run ahead of a
/// slow block while the caller still delivers in input order.
///
/// Memory: each thread that encodes (N workers including the caller, plus
/// at most N lent helpers) owns one `Scratch`, whose trial output stays below
/// 4 MiB; each of the at most 4N slots holds a block of input and that
/// block's selected output, at most 17n/8 + 6060 bytes for n input bytes.
const SLOTS_PER_THREAD: usize = 4;

/// A block's selected bits and CRCs, copied out of the encoding thread's
/// trial scratch so that scratch can take the next block at once.
#[derive(Default)]
struct Encoded {
    bytes: Vec<u8>,
    bits: usize,
    crcs: Vec<u32>,
}

impl Encoded {
    fn copy_from(&mut self, trial: &TrialWriter) -> io::Result<()> {
        let output = trial.output();
        self.bytes.clear();
        self.bytes
            .try_reserve(output.len())
            .map_err(io::Error::other)?;
        self.bytes.extend_from_slice(output);
        self.crcs.clear();
        self.crcs
            .try_reserve(trial.crcs().len())
            .map_err(io::Error::other)?;
        self.crcs.extend_from_slice(trial.crcs());
        self.bits = trial.bit_len();
        Ok(())
    }
}

struct Slot {
    input: Vec<u8>,
    encoded: Encoded,
}

impl Slot {
    fn new(block_limit: usize) -> io::Result<Self> {
        let mut input = Vec::new();
        input
            .try_reserve_exact(block_limit + 1)
            .map_err(io::Error::other)?;
        Ok(Self {
            input,
            encoded: Encoded::default(),
        })
    }

    fn encode(
        &mut self,
        scratch: &mut Scratch,
        passes: u32,
        helpers: Option<&Helpers>,
    ) -> io::Result<()> {
        scratch.trial.reset();
        scratch.tree(&self.input, passes, passes > 1, helpers)?;
        self.encoded.copy_from(&scratch.trial)
    }
}

struct Job {
    sequence: usize,
    slot: Slot,
    passes: u32,
    helpers: Option<Arc<Helpers>>,
    #[cfg(test)]
    gate: Option<Arc<std::sync::Barrier>>,
    #[cfg(test)]
    panic: bool,
}

struct Completed {
    sequence: usize,
    slot: Slot,
    result: io::Result<()>,
}

fn execute(mut job: Job, owned: &mut Vec<Scratch>) -> Completed {
    let helpers = job.helpers.take();
    if let Some(helpers) = &helpers {
        helpers.idle.fetch_sub(1, Ordering::AcqRel);
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        if let Some(gate) = &job.gate {
            gate.wait();
        }
        #[cfg(test)]
        assert!(!job.panic, "injected encoder job panic");
        let scratch = owned_scratch(owned)?;
        job.slot.encode(scratch, job.passes, helpers.as_deref())
    }))
    .unwrap_or_else(|_| {
        // Scratch interrupted by a panic is discarded rather than reused.
        owned.clear();
        Err(io::Error::other("BZip2 worker panicked"))
    });
    if let Some(helpers) = &helpers {
        helpers.idle.fetch_add(1, Ordering::AcqRel);
    }
    Completed {
        sequence: job.sequence,
        slot: job.slot,
        result,
    }
}

struct Pool {
    jobs: Option<SyncSender<Job>>,
    results: Option<Receiver<Completed>>,
    input: Arc<Mutex<Receiver<Job>>>,
    workers: Vec<JoinHandle<()>>,
    cancelled: Arc<AtomicBool>,
    helpers: Arc<Helpers>,
}

impl Pool {
    fn new(threads: usize, slots: usize, lending: bool) -> io::Result<Self> {
        // Every open slot can wait queued, so submitting never blocks.
        let (jobs, input) = mpsc::sync_channel::<Job>(slots);
        let input = Arc::new(Mutex::new(input));
        let (output, results) = mpsc::channel();
        let mut pool = Self {
            jobs: Some(jobs),
            results: Some(results),
            input: input.clone(),
            workers: Vec::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
            helpers: Arc::new(Helpers::new(threads, lending)),
        };
        pool.workers
            .try_reserve_exact(threads - 1)
            .map_err(io::Error::other)?;
        for _ in 1..threads {
            let input = input.clone();
            let output = output.clone();
            let cancelled = pool.cancelled.clone();
            let worker = thread::Builder::new().spawn(move || {
                let mut scratch = Vec::new();
                loop {
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let job = match input.lock() {
                        Ok(input) => input.recv(),
                        Err(_) => break,
                    };
                    let Ok(job) = job else {
                        break;
                    };
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let completed = execute(job, &mut scratch);
                    let failed = completed.result.is_err();
                    if output.send(completed).is_err() || failed {
                        break;
                    }
                }
            })?;
            // The pool already owns all earlier handles if spawning fails.
            pool.workers.push(worker);
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

#[derive(Default)]
struct StreamBits {
    pending: u8,
    count: u8,
}

impl StreamBits {
    fn append<W: Write>(&mut self, bytes: &[u8], bits: usize, output: &mut W) -> io::Result<()> {
        let full = bits / 8;
        if self.count == 0 {
            output.write_all(&bytes[..full])?;
        } else {
            let shift = self.count;
            let mask = (1u8 << shift) - 1;
            let mut buffer = [0u8; 8192];
            for chunk in bytes[..full].chunks(buffer.len()) {
                let whole = chunk.len() / 8 * 8;
                for (source, dest) in chunk[..whole]
                    .chunks_exact(8)
                    .zip(buffer[..whole].chunks_exact_mut(8))
                {
                    let word = u64::from_be_bytes(source.try_into().unwrap());
                    let packed = (u64::from(self.pending) << (64 - shift)) | (word >> shift);
                    dest.copy_from_slice(&packed.to_be_bytes());
                    self.pending = word as u8 & mask;
                }
                for (dest, &byte) in buffer[whole..].iter_mut().zip(&chunk[whole..]) {
                    *dest = (self.pending << (8 - shift)) | (byte >> shift);
                    self.pending = byte & mask;
                }
                output.write_all(&buffer[..chunk.len()])?;
            }
        }
        let count = (bits & 7) as u8;
        if count != 0 {
            let combined =
                (u16::from(self.pending) << count) | u16::from(bytes[full] >> (8 - count));
            let mut remaining = self.count + count;
            if remaining >= 8 {
                remaining -= 8;
                output.write_all(&[(combined >> remaining) as u8])?;
            }
            self.pending = (combined & ((1 << remaining) - 1)) as u8;
            self.count = remaining;
        }
        Ok(())
    }
}

/// A block-bounded native BZip2 [`Write`] encoder.
///
/// [`Write::flush`] drains previously submitted blocks and flushes the underlying
/// writer, but preserves the partially filled block and pending run. Call
/// [`Self::finish`] to submit that block and write the trailer. Dropping an encoder
/// joins its workers but does not complete the stream or report pending I/O errors.
///
/// With multiple threads, completed blocks are delivered in input order. Write
/// errors poison the encoder; later operations return an error.
pub struct Encoder<W: Write> {
    output: Option<W>,
    options: EncoderOptions,
    stream: StreamBits,
    header: [u8; 4],
    header_sent: bool,
    combined_crc: u32,
    run_byte: u8,
    run_count: usize,
    block_limit: usize,
    // Every allocated slot is exactly one of free, filling, or pending. Queued,
    // executing and completed jobs all occupy their existing pending entry.
    // At most `max_slots` are allocated: one serially, otherwise
    // `SLOTS_PER_THREAD` per thread of the budget.
    max_slots: usize,
    free: Vec<Slot>,
    filling: Option<Slot>,
    allocated: usize,
    pending: VecDeque<Option<Slot>>,
    output_sequence: usize,
    pool: Option<Pool>,
    // The caller's own scratch, for serial blocks and queued jobs it runs.
    scratch: Vec<Scratch>,
    // Whether the pool may lend threads to trial halves; see `drain`.
    lending: bool,
    failure: bool,
}

impl<W: Write> Encoder<W> {
    /// Creates an encoder, normalizing options as documented by [`EncoderOptions`].
    /// Output and workers are initialized lazily as input is written.
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
            .unwrap_or(match options.level {
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
                    9 => 2,
                    7 | 8 => 2,
                    _ => 1,
                })
                .clamp(1, 10),
        );
        let max_slots = if options.threads == 1 {
            1
        } else {
            options.threads * SLOTS_PER_THREAD
        };
        Self {
            output: Some(output),
            options,
            stream: StreamBits::default(),
            header: [b'B', b'Z', b'h', b'0' + multiplier as u8],
            header_sent: false,
            combined_crc: 0,
            run_byte: 0,
            run_count: 0,
            block_limit: multiplier * 100_000 - 1,
            max_slots,
            free: Vec::new(),
            filling: None,
            allocated: 0,
            pending: VecDeque::new(),
            output_sequence: 0,
            pool: None,
            scratch: Vec::new(),
            lending: false,
            failure: false,
        }
    }

    /// Borrows the output writer. Pending codec bytes may not yet be written.
    pub fn get_ref(&self) -> &W {
        self.output.as_ref().unwrap()
    }
    /// Mutably borrows the output writer.
    /// Writing directly to it can corrupt the BZip2 stream; this does not flush
    /// or finish pending codec output.
    pub fn get_mut(&mut self) -> &mut W {
        self.output.as_mut().unwrap()
    }

    fn header(&mut self) -> io::Result<()> {
        if !self.header_sent {
            self.output.as_mut().unwrap().write_all(&self.header)?;
            self.header_sent = true;
        }
        Ok(())
    }

    fn fail(&mut self) {
        self.failure = true;
        drop(self.pool.take());
        self.pending.clear();
        self.filling = None;
        self.free.clear();
        self.scratch.clear();
        self.allocated = 0;
    }

    fn guarded<T>(&mut self, action: impl FnOnce(&mut Self) -> io::Result<T>) -> io::Result<T> {
        if self.failure {
            return Err(io::Error::other("BZip2 encoder is in a failed state"));
        }
        match catch_unwind(AssertUnwindSafe(|| action(self))) {
            Ok(result) => {
                if result.is_err() {
                    self.fail();
                }
                result
            }
            Err(payload) => {
                self.fail();
                resume_unwind(payload)
            }
        }
    }

    fn check_slots(&self) {
        debug_assert_eq!(
            self.allocated,
            self.free.len() + usize::from(self.filling.is_some()) + self.pending.len()
        );
        debug_assert!(self.allocated <= self.max_slots);
    }

    fn acquire(&mut self) -> io::Result<()> {
        if self.filling.is_some() {
            return Ok(());
        }
        self.harvest()?;
        while self.free.is_empty() && self.allocated == self.max_slots {
            self.wait_first()?;
        }
        let mut slot = if let Some(slot) = self.free.pop() {
            slot
        } else {
            self.free
                .try_reserve_exact(self.max_slots)
                .map_err(io::Error::other)?;
            self.pending
                .try_reserve(self.max_slots)
                .map_err(io::Error::other)?;
            let slot = Slot::new(self.block_limit)?;
            self.allocated += 1;
            slot
        };
        slot.input.clear();
        self.filling = Some(slot);
        self.check_slots();
        Ok(())
    }

    fn emit(&mut self, bytes: &[u8], bits: usize, crcs: &[u32]) -> io::Result<()> {
        self.header()?;
        self.stream
            .append(bytes, bits, self.output.as_mut().unwrap())?;
        for &crc in crcs {
            self.combined_crc = self.combined_crc.rotate_left(1) ^ crc;
        }
        Ok(())
    }

    fn accept(&mut self, mut slot: Slot) -> io::Result<()> {
        let encoded = &slot.encoded;
        self.emit(&encoded.bytes, encoded.bits, &encoded.crcs)?;
        slot.input.clear();
        self.free.push(slot);
        self.check_slots();
        Ok(())
    }

    fn store(&mut self, completed: Completed) -> io::Result<()> {
        completed.result?;
        let offset = completed
            .sequence
            .checked_sub(self.output_sequence)
            .ok_or_else(|| io::Error::other("BZip2 result preceded output"))?;
        let slot = self
            .pending
            .get_mut(offset)
            .ok_or_else(|| io::Error::other("BZip2 result exceeded pending slots"))?;
        if slot.is_some() {
            return Err(io::Error::other("duplicate BZip2 result"));
        }
        *slot = Some(completed.slot);
        Ok(())
    }

    fn wait_first(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        while self.pending.front().is_some_and(Option::is_none) {
            if let Some(job) = self.pool.as_ref().unwrap().take_job() {
                let completed = execute(job, &mut self.scratch);
                self.store(completed)?;
                continue;
            }
            let completed = self
                .pool
                .as_ref()
                .unwrap()
                .results
                .as_ref()
                .unwrap()
                .recv()
                .map_err(|_| io::Error::other("BZip2 result channel closed"))?;
            self.store(completed)?;
        }
        let next_sequence = self
            .output_sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("BZip2 output sequence overflow"))?;
        let slot = self.pending.pop_front().unwrap().unwrap();
        self.output_sequence = next_sequence;
        self.accept(slot)
    }

    fn harvest(&mut self) -> io::Result<()> {
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
                    Ok(completed) => self.store(completed)?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if self.pending.iter().any(Option::is_none) {
                            return Err(io::Error::other("BZip2 result channel closed"));
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
        if self
            .filling
            .as_ref()
            .is_none_or(|slot| slot.input.is_empty())
        {
            return Ok(());
        }
        if self.options.threads == 1 {
            // Serial blocks are written straight from the caller's trial.
            let mut slot = self.filling.take().unwrap();
            let mut owned = std::mem::take(&mut self.scratch);
            let scratch = owned_scratch(&mut owned)?;
            scratch.trial.reset();
            let passes = self.options.passes.unwrap();
            scratch.tree(&slot.input, passes, passes > 1, None)?;
            let trial = &scratch.trial;
            self.emit(trial.output(), trial.bit_len(), trial.crcs())?;
            self.scratch = owned;
            slot.input.clear();
            self.free.push(slot);
        } else {
            if self.pool.is_none() {
                self.pool = Some(Pool::new(
                    self.options.threads,
                    self.max_slots,
                    self.lending,
                )?);
            }
            let sequence = self
                .output_sequence
                .checked_add(self.pending.len())
                .ok_or_else(|| io::Error::other("BZip2 block sequence overflow"))?;
            let slot = self.filling.take().unwrap();
            self.pending.push_back(None);
            let job = Job {
                sequence,
                slot,
                passes: self.options.passes.unwrap(),
                helpers: Some(Arc::clone(&self.pool.as_ref().unwrap().helpers)),
                #[cfg(test)]
                gate: None,
                #[cfg(test)]
                panic: false,
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
                    let completed = execute(job, &mut self.scratch);
                    self.store(completed)?;
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(io::Error::other("BZip2 job channel closed"));
                }
            }
        }
        self.check_slots();
        Ok(())
    }

    fn emit_run(&mut self) {
        if self.run_count >= 4 {
            self.filling
                .as_mut()
                .unwrap()
                .input
                .push((self.run_count - 4) as u8);
        }
        self.run_count = 0;
    }

    fn write_inner(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut position = 0;
        while position < input.len() {
            if self.filling.is_none() {
                self.acquire()?;
            }
            if self.filling.as_ref().unwrap().input.len() >= self.block_limit {
                self.emit_run();
                self.submit()?;
                self.acquire()?;
            }
            if self.run_count >= 4 && input[position] == self.run_byte {
                let limit = (259 - self.run_count).min(input.len() - position);
                let count = repeated_prefix(&input[position..position + limit], self.run_byte);
                position += count;
                self.run_count += count;
                if self.run_count == 259 {
                    self.filling.as_mut().unwrap().input.push(255);
                    self.run_count = 0;
                }
                continue;
            }
            let byte = input[position];
            if self.run_count == 0 || byte != self.run_byte {
                self.emit_run();
                let available = self
                    .block_limit
                    .saturating_sub(self.filling.as_ref().unwrap().input.len());
                let limit = available.min(input.len() - position);
                // A previous long run's count can occupy the last permitted
                // byte. The reference still admits this first new literal,
                // giving its documented block_limit+1 result.
                let length = literal_prefix(&input[position..position + limit]).max(1);
                let end = position + length;
                self.filling
                    .as_mut()
                    .unwrap()
                    .input
                    .extend_from_slice(&input[position..end]);
                self.run_byte = input[end - 1];
                let mut repeats = 1;
                while repeats < 3 && repeats < length && input[end - repeats - 1] == self.run_byte {
                    repeats += 1;
                }
                self.run_count = repeats;
                position = end;
            } else {
                position += 1;
                self.run_count += 1;
                if self.run_count <= 4 {
                    self.filling.as_mut().unwrap().input.push(byte);
                } else if self.run_count == 259 {
                    self.filling.as_mut().unwrap().input.push(255);
                    self.run_count = 0;
                }
            }
        }
        self.check_slots();
        Ok(input.len())
    }

    #[cfg(test)]
    fn write_inner_checked(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut position = 0;
        while position < input.len() {
            if self.filling.is_none() {
                self.acquire()?;
            }
            if self.filling.as_ref().unwrap().input.len() >= self.block_limit {
                self.emit_run();
                self.submit()?;
                self.acquire()?;
            }
            if self.run_count >= 4 && input[position] == self.run_byte {
                let limit = (259 - self.run_count).min(input.len() - position);
                let count = repeated_prefix(&input[position..position + limit], self.run_byte);
                position += count;
                self.run_count += count;
                if self.run_count == 259 {
                    self.filling.as_mut().unwrap().input.push(255);
                    self.run_count = 0;
                }
                continue;
            }
            let byte = input[position];
            position += 1;
            if self.run_count == 0 || byte != self.run_byte {
                self.emit_run();
                self.filling.as_mut().unwrap().input.push(byte);
                self.run_byte = byte;
                self.run_count = 1;
            } else {
                self.run_count += 1;
                if self.run_count <= 4 {
                    self.filling.as_mut().unwrap().input.push(byte);
                } else if self.run_count == 259 {
                    self.filling.as_mut().unwrap().input.push(255);
                    self.run_count = 0;
                }
            }
        }
        self.check_slots();
        Ok(input.len())
    }

    /// Ends the current block without ending the stream for the raw adapter.
    pub(crate) fn flush_block(&mut self) -> io::Result<()> {
        self.guarded(|this| {
            this.emit_run();
            this.drain(true)?;
            this.header()
        })
    }

    /// Submits the filling block when `submit` is set, then waits for every
    /// pending block. No block follows until this returns, so a thread that
    /// finishes its block is truly spare; only here may the pool lend it to
    /// the remaining blocks' trial halves.
    fn drain(&mut self, submit: bool) -> io::Result<()> {
        self.lend(true);
        let result = self.drain_pending(submit);
        self.lend(false);
        result
    }

    fn drain_pending(&mut self, submit: bool) -> io::Result<()> {
        if submit {
            self.submit()?;
        }
        while !self.pending.is_empty() {
            self.wait_first()?;
        }
        Ok(())
    }

    fn lend(&mut self, lending: bool) {
        self.lending = lending;
        if let Some(pool) = &self.pool {
            pool.helpers.lending.store(lending, Ordering::Release);
        }
    }

    /// Completes the stream, joins pending work and returns the flushed writer.
    ///
    /// Submits the current block, writes the combined-checksum trailer and final
    /// padding, then calls [`Write::flush`] on the underlying writer. Returns
    /// worker or output errors instead of silently discarding them.
    pub fn finish(mut self) -> io::Result<W> {
        self.guarded(|this| {
            this.emit_run();
            this.drain(true)?;
            this.header()?;
            let crc = this.combined_crc.to_be_bytes();
            let footer = [
                0x17, 0x72, 0x45, 0x38, 0x50, 0x90, crc[0], crc[1], crc[2], crc[3],
            ];
            let output = this.output.as_mut().unwrap();
            this.stream.append(&footer, 80, output)?;
            if this.stream.count != 0 {
                output.write_all(&[this.stream.pending << (8 - this.stream.count)])?;
            }
            output.flush()
        })?;
        Ok(self.output.take().unwrap())
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        self.guarded(|this| this.write_inner(input))
    }
    fn flush(&mut self) -> io::Result<()> {
        self.guarded(|this| {
            this.drain(false)?;
            this.header()?;
            this.output.as_mut().unwrap().flush()
        })
    }
}

impl<W: Write> Drop for Encoder<W> {
    fn drop(&mut self) {
        drop(self.pool.take());
    }
}

// The maximal prefix preceding the first four-equal-byte run. Every byte in
// that prefix is emitted verbatim by CRleEncoder::Encode; trailing run state
// is therefore at most three. Explicit little endian preserves byte order
// for the least-zero-byte detection on every architecture.
fn literal_prefix(input: &[u8]) -> usize {
    let mut fourth = 3;
    while fourth + 8 <= input.len() {
        let current = u64::from_le_bytes(input[fourth..fourth + 8].try_into().unwrap());
        let previous = u64::from_le_bytes(input[fourth - 1..fourth + 7].try_into().unwrap());
        let previous2 = u64::from_le_bytes(input[fourth - 2..fourth + 6].try_into().unwrap());
        let previous3 = u64::from_le_bytes(input[fourth - 3..fourth + 5].try_into().unwrap());
        let different = (current ^ previous) | (current ^ previous2) | (current ^ previous3);
        let matches = different.wrapping_sub(0x0101010101010101) & !different & 0x8080808080808080;
        if matches != 0 {
            return fourth + matches.trailing_zeros() as usize / 8 - 3;
        }
        fourth += 8;
    }
    while fourth < input.len() {
        if input[fourth] == input[fourth - 1]
            && input[fourth] == input[fourth - 2]
            && input[fourth] == input[fourth - 3]
        {
            return fourth - 3;
        }
        fourth += 1;
    }
    input.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(bits: &[bool]) -> Vec<u8> {
        bits.chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0, |v, (i, &b)| v | (u8::from(b) << (7 - i)))
            })
            .collect()
    }

    #[test]
    fn streaming_carry_matches_independent_bits_at_each_chunk_boundary() {
        for alignment in 0..8 {
            for partial in 0..8 {
                let mut stream = StreamBits::default();
                let mut output = Vec::new();
                let mut expected = Vec::new();
                for size in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 8191, 8192, 8193, 0, 16385] {
                    let bit_count = size * 8 + if size == 0 { alignment } else { partial };
                    let logical: Vec<_> = (0..bit_count)
                        .map(|i| (i * 17 + size + partial) % 7 < 3)
                        .collect();
                    stream
                        .append(&bytes(&logical), bit_count, &mut output)
                        .unwrap();
                    expected.extend(logical);
                    let full = expected.len() / 8;
                    assert_eq!(output, bytes(&expected[..full * 8]));
                    assert_eq!(usize::from(stream.count), expected.len() % 8);
                    let pending = expected[full * 8..]
                        .iter()
                        .fold(0, |v, &b| (v << 1) | u8::from(b));
                    assert_eq!(stream.pending, pending);
                }
            }
        }
    }

    #[test]
    fn streaming_word_carry_preserves_written_prefix_on_sink_failure() {
        struct RefuseAfter {
            bytes: Vec<u8>,
            limit: usize,
        }
        impl Write for RefuseAfter {
            fn write(&mut self, input: &[u8]) -> io::Result<usize> {
                let take = input.len().min(self.limit - self.bytes.len());
                if take == 0 {
                    return Err(io::Error::other("injected sink failure"));
                }
                self.bytes.extend_from_slice(&input[..take]);
                Ok(take)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        for alignment in 0..8 {
            let prefix: Vec<_> = (0..alignment).map(|i| i % 2 == 0).collect();
            let logical: Vec<_> = (0..8194 * 8 + 7).map(|i| i % 11 < 5).collect();
            let expected = bytes(&[prefix.clone(), logical.clone()].concat());
            for limit in [0, 1, 7, 8, 8191, 8192, 8193] {
                let mut sink = RefuseAfter {
                    bytes: Vec::new(),
                    limit,
                };
                let mut stream = StreamBits::default();
                stream
                    .append(&bytes(&prefix), alignment, &mut sink)
                    .unwrap();
                assert!(
                    stream
                        .append(&bytes(&logical), logical.len(), &mut sink)
                        .is_err()
                );
                assert_eq!(sink.bytes, expected[..limit]);
            }
        }
    }

    fn capacities(slot: &Slot) -> (usize, usize) {
        (slot.input.capacity(), slot.encoded.bytes.capacity())
    }

    fn scratch_capacities(scratch: &[Scratch]) -> Option<(usize, usize, usize)> {
        scratch.first().map(|work| {
            (
                work.sort.capacity(),
                work.entropy.capacity(),
                work.trial.capacity(),
            )
        })
    }

    #[test]
    fn slots_include_filling_and_reuse_the_same_storage_large_to_small() {
        for budget in [1, 2, 4] {
            let mut encoder = Encoder::new(
                Vec::new(),
                EncoderOptions {
                    threads: budget,
                    passes: Some(1),
                    ..Default::default()
                },
            );
            for size in [65537, 11, 65537, 257, 4097] {
                encoder.acquire().unwrap();
                assert_eq!(
                    encoder.allocated,
                    encoder.free.len() + encoder.pending.len() + 1
                );
                assert!(encoder.allocated <= encoder.max_slots);
                let slot = encoder.filling.as_mut().unwrap();
                slot.input.extend((0..size).map(|i| (i % 251) as u8));
                encoder.submit().unwrap();
                encoder.flush().unwrap();
                assert_eq!(encoder.free.len(), encoder.allocated);
                assert!(encoder.pending.is_empty());
            }
            assert_eq!(
                encoder.pool.as_ref().map_or(0, |p| p.workers.len()),
                encoder.options.threads - 1
            );
            let expected_slots = if budget == 1 {
                1
            } else {
                budget * SLOTS_PER_THREAD
            };
            assert_eq!(encoder.max_slots, expected_slots);
            // Threads own the scratch; slots keep only input and encoded
            // output. The serial caller has encoded every block itself.
            assert!(budget > 1 || !encoder.scratch.is_empty());
            let scratch = scratch_capacities(&encoder.scratch);
            let slot = &encoder.free[0];
            let capacity = capacities(slot);
            let identity = slot.input.as_ptr();
            encoder.acquire().unwrap();
            assert_eq!(encoder.filling.as_ref().unwrap().input.as_ptr(), identity);
            encoder
                .filling
                .as_mut()
                .unwrap()
                .input
                .extend_from_slice(b"small final block");
            encoder.submit().unwrap();
            encoder.flush().unwrap();
            assert_eq!(capacities(&encoder.free[0]), capacity);
            assert_eq!(scratch_capacities(&encoder.scratch), scratch);
            encoder.finish().unwrap();
        }
    }

    #[test]
    fn new_slots_match_checked_encoder_across_blocks_passes_and_flushes() {
        let mut state = 0x1923_ab5du32;
        let input: Vec<_> = (0..310017)
            .map(|i| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                if i % 997 < 300 {
                    (i % 17) as u8
                } else {
                    state as u8
                }
            })
            .collect();
        for budget in [1, 2, 4] {
            for passes in [1, 2, 7, 10] {
                let options = EncoderOptions {
                    threads: budget,
                    block_size: Some(100000),
                    passes: Some(passes),
                    ..Default::default()
                };
                let mut old = super::super::baseline::Encoder::new(Vec::new(), options.clone());
                assert!(old.get_ref().is_empty());
                assert!(old.get_mut().is_empty());
                let mut new = Encoder::new(Vec::new(), options);
                for chunk in input.chunks(65537) {
                    old.write_all(chunk).unwrap();
                    new.write_all(chunk).unwrap();
                    assert_eq!(
                        new.allocated,
                        new.free.len() + new.pending.len() + usize::from(new.filling.is_some())
                    );
                    assert!(new.allocated <= new.max_slots);
                    old.flush().unwrap();
                    new.flush().unwrap();
                    assert_eq!(new.write(&[]).unwrap(), 0);
                }
                assert_eq!(
                    new.finish().unwrap(),
                    old.finish().unwrap(),
                    "budget={budget},passes={passes}"
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn helped_trials_match_serial_trials_byte_for_byte() {
        let mut state = 0x2545_f491u32;
        let mut noise = |len: usize, alphabet: u32| -> Vec<u8> {
            (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    (state % alphabet) as u8
                })
                .collect()
        };
        let text: Vec<u8> = b"{\"id\": 17, \"name\": \"bzip2\", \"tags\": [\"a\", \"bb\"]}\n"
            .iter()
            .copied()
            .cycle()
            .take(400_000)
            .zip(noise(400_000, 3))
            .map(|(byte, jitter)| byte.wrapping_add(jitter * (byte & 1)))
            .collect();
        let inputs = [
            noise(899_999, 256),
            noise(300_000, 4),
            text,
            noise(70_000, 50),
            noise(5_000, 9),
        ];
        let mut serial = Scratch::new();
        for (index, input) in inputs.iter().enumerate() {
            let passes: &[u32] = if index == 3 { &[2, 3, 10] } else { &[2, 7] };
            for &passes in passes {
                for threads in [2, 16] {
                    let helpers = Helpers::new(threads, true);
                    serial.trial.reset();
                    serial.tree(input, passes, true, None).unwrap();
                    let mut helped = Scratch::new();
                    helped.tree(input, passes, true, Some(&helpers)).unwrap();
                    let label = format!("len={},passes={passes},threads={threads}", input.len());
                    assert!(helped.trial.output() == serial.trial.output(), "{label}");
                    assert_eq!(helped.trial.bit_len(), serial.trial.bit_len(), "{label}");
                    assert_eq!(helped.trial.crcs(), serial.trial.crcs(), "{label}");
                    // Every lent thread came back, and halves large enough
                    // to split did run on helpers.
                    assert_eq!(helpers.idle.load(Ordering::Acquire), threads as isize);
                    let spare = helpers.spare.lock().unwrap().len();
                    assert!(spare <= threads, "{label}");
                    assert_eq!(spare > 0, input.len() >= 2 * MIN_HELPED_INPUT, "{label}");
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_helper_panic_fails_the_block_and_returns_the_thread() {
        let helpers = Arc::new(Helpers::new(4, true));
        let mut slot = job(0, &[0u8; 4]).slot;
        slot.input.clear();
        // A helped block encodes and returns every lent thread.
        slot.input.extend((0..100_000u32).map(|i| (i % 251) as u8));
        let mut good = Job {
            helpers: Some(Arc::clone(&helpers)),
            passes: 7,
            ..job(1, &[])
        };
        good.slot = slot;
        let completed = execute(good, &mut Vec::new());
        assert!(completed.result.is_ok());
        assert_eq!(helpers.idle.load(Ordering::Acquire), 4);
        let mut bad = job(2, b"panicking block");
        bad.helpers = Some(Arc::clone(&helpers));
        bad.panic = true;
        assert!(execute(bad, &mut Vec::new()).result.is_err());
        assert_eq!(helpers.idle.load(Ordering::Acquire), 4);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn threads_are_lent_only_while_the_encoder_drains() {
        // Closed lending lends nothing and leaves the count untouched.
        let closed = Helpers::new(4, false);
        assert!(closed.claim().is_none());
        assert_eq!(closed.idle.load(Ordering::Acquire), 4);
        // Blocks submitted while writing do not open it; a drain opens it
        // and closes it again, also when it creates the pool itself.
        let input: Vec<u8> = (0..450_000u32).map(|i| (i * 7 % 253) as u8).collect();
        let options = EncoderOptions {
            threads: 4,
            block_size: Some(100_000),
            passes: Some(3),
            ..Default::default()
        };
        let mut writing = Encoder::new(Vec::new(), options.clone());
        writing.write_all(&input).unwrap();
        let lending = |encoder: &Encoder<Vec<u8>>| {
            encoder
                .pool
                .as_ref()
                .unwrap()
                .helpers
                .lending
                .load(Ordering::Acquire)
        };
        assert!(!lending(&writing));
        writing.flush().unwrap();
        assert!(!writing.lending && !lending(&writing));
        let mut small = Encoder::new(Vec::new(), options.clone());
        small.write_all(&input[..60_000]).unwrap();
        assert!(small.pool.is_none());
        small.flush_block().unwrap();
        assert!(!small.lending && !lending(&small));
        // Lending changes no byte.
        let serial = Encoder::new(
            Vec::new(),
            EncoderOptions {
                threads: 1,
                ..options
            },
        );
        let mut serial = serial;
        serial.write_all(&input).unwrap();
        assert!(writing.finish().unwrap() == serial.finish().unwrap());
    }

    fn job(sequence: usize, input: &[u8]) -> Job {
        let mut slot = Slot::new(100000).unwrap();
        slot.input.extend_from_slice(input);
        Job {
            sequence,
            slot,
            passes: 1,
            helpers: None,
            gate: None,
            panic: false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn real_worker_out_of_order_results_wait_for_ordered_delivery() {
        let mut encoder = Encoder::new(
            Vec::new(),
            EncoderOptions {
                threads: 2,
                passes: Some(1),
                ..Default::default()
            },
        );
        let pool = Pool::new(2, 2, false).unwrap();
        let gate = Arc::new(std::sync::Barrier::new(2));
        let mut first = job(0, b"first owned block");
        first.gate = Some(gate.clone());
        assert!(pool.jobs.as_ref().unwrap().send(first).is_ok());
        encoder.pool = Some(pool);
        encoder.pending.push_back(None);
        encoder.pending.push_back(None);
        encoder.allocated = 2;
        encoder
            .store(execute(job(1, b"second owned block"), &mut Vec::new()))
            .unwrap();
        assert!(encoder.get_ref().is_empty());
        assert!(encoder.pending[0].is_none());
        assert!(encoder.pending[1].is_some());
        gate.wait();
        encoder.wait_first().unwrap();
        encoder.wait_first().unwrap();
        assert_eq!(encoder.output_sequence, 2);
        assert_eq!(encoder.free.len(), 2);
        encoder.finish().unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn worker_and_caller_panics_report_sequence_and_cancel_queued_slots() {
        let pool = Pool::new(2, 2, false).unwrap();
        let mut bad = job(17, b"panic block");
        bad.panic = true;
        assert!(pool.jobs.as_ref().unwrap().send(bad).is_ok());
        let result = pool
            .results
            .as_ref()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(result.sequence, 17);
        assert!(result.result.is_err());
        for sequence in 18..20 {
            assert!(
                pool.jobs
                    .as_ref()
                    .unwrap()
                    .send(job(sequence, b"queued owned block"))
                    .is_ok()
            );
        }
        let input = pool.input.clone();
        drop(pool);
        assert!(matches!(
            input.lock().unwrap().try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        let mut bad = job(99, b"caller help panic");
        bad.panic = true;
        let mut scratch = Vec::new();
        let result = execute(bad, &mut scratch);
        assert!(scratch.is_empty());
        assert_eq!(result.sequence, 99);
        assert!(result.result.is_err());
    }

    struct Sink {
        bytes: Vec<u8>,
        fail: bool,
        panic: bool,
        interrupted: bool,
        flush_fail: bool,
    }
    impl Write for Sink {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            assert!(!self.panic, "injected sink panic");
            if self.fail {
                return Err(io::Error::other("injected sink error"));
            }
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            let size = input.len().min(3);
            self.bytes.extend_from_slice(&input[..size]);
            Ok(size)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.flush_fail {
                Err(io::Error::other("injected flush error"))
            } else {
                Ok(())
            }
        }
    }
    fn sink() -> Sink {
        Sink {
            bytes: Vec::new(),
            fail: false,
            panic: false,
            interrupted: false,
            flush_fail: false,
        }
    }

    #[test]
    fn errors_and_sink_unwind_join_and_poison_all_slots() {
        for mode in 0..3 {
            let mut encoder = Encoder::new(
                sink(),
                EncoderOptions {
                    threads: 2,
                    passes: Some(1),
                    ..Default::default()
                },
            );
            encoder.acquire().unwrap();
            encoder
                .filling
                .as_mut()
                .unwrap()
                .input
                .extend_from_slice(b"owned pending block");
            encoder.submit().unwrap();
            match mode {
                0 => encoder.get_mut().fail = true,
                1 => encoder.get_mut().flush_fail = true,
                _ => encoder.get_mut().panic = true,
            }
            let result = catch_unwind(AssertUnwindSafe(|| encoder.flush()));
            if mode == 2 {
                assert!(result.is_err());
            } else {
                assert!(result.unwrap().is_err());
            }
            assert!(encoder.failure);
            assert!(encoder.pool.is_none());
            assert_eq!(encoder.allocated, 0);
            assert!(
                encoder.free.is_empty() && encoder.filling.is_none() && encoder.pending.is_empty()
            );
            assert!(encoder.write(b"later").is_err());
            assert!(encoder.flush().is_err());
        }
    }

    #[test]
    fn short_interrupted_writes_empty_input_and_mid_run_flush_preserve_stream() {
        let input = vec![42; 10000];
        let mut encoder = Encoder::new(sink(), EncoderOptions::default());
        assert_eq!(encoder.write(&[]).unwrap(), 0);
        assert_eq!(encoder.allocated, 0);
        for part in input.chunks(137) {
            encoder.write_all(part).unwrap();
            encoder.flush().unwrap();
        }
        let actual = encoder.finish().unwrap().bytes;
        let mut expected =
            super::super::baseline::Encoder::new(Vec::new(), EncoderOptions::default());
        expected.write_all(&input).unwrap();
        assert_eq!(actual, expected.finish().unwrap());
        let mut empty = Encoder::new(Vec::new(), EncoderOptions::default());
        empty.flush().unwrap();
        assert_eq!(empty.allocated, 0);
        let expected = super::super::baseline::Encoder::new(Vec::new(), EncoderOptions::default())
            .finish()
            .unwrap();
        assert_eq!(empty.finish().unwrap(), expected);
    }

    #[test]
    fn retirement_overflow_is_a_sticky_error_without_writing_the_slot() {
        let mut encoder = Encoder::new(Vec::new(), EncoderOptions::default());
        let mut slot = job(0, b"retirement overflow block").slot;
        slot.encode(&mut Scratch::new(), 1, None).unwrap();
        encoder.allocated = 1;
        encoder.pending.push_back(Some(slot));
        encoder.output_sequence = usize::MAX;
        assert!(encoder.flush().is_err());
        assert!(encoder.failure);
        assert!(encoder.get_ref().is_empty());
        assert_eq!(encoder.allocated, 0);
        assert!(encoder.pending.is_empty());
        assert!(encoder.write(b"later").is_err());
    }

    fn same_admission(new: &Encoder<Vec<u8>>, old: &Encoder<Vec<u8>>) {
        assert_eq!(new.run_byte, old.run_byte);
        assert_eq!(new.run_count, old.run_count);
        assert_eq!(new.combined_crc, old.combined_crc);
        assert_eq!(new.stream.pending, old.stream.pending);
        assert_eq!(new.stream.count, old.stream.count);
        assert_eq!(new.get_ref(), old.get_ref());
        assert_eq!(
            new.filling.as_ref().map(|s| s.input.as_slice()),
            old.filling.as_ref().map(|s| s.input.as_slice())
        );
    }

    fn both(new: &mut Encoder<Vec<u8>>, old: &mut Encoder<Vec<u8>>, input: &[u8]) {
        assert_eq!(new.write(input).unwrap(), input.len());
        assert_eq!(
            old.guarded(|this| this.write_inner_checked(input)).unwrap(),
            input.len()
        );
        same_admission(new, old);
    }

    fn pair() -> (Encoder<Vec<u8>>, Encoder<Vec<u8>>) {
        let options = EncoderOptions {
            threads: 1,
            passes: Some(1),
            ..Default::default()
        };
        (
            Encoder::new(Vec::new(), options.clone()),
            Encoder::new(Vec::new(), options),
        )
    }

    #[test]
    fn literal_word_scan_matches_quad_windows_at_every_alignment_and_edge() {
        let mut state = 0x1923_ab5du32;
        for offset in 0..16 {
            for size in 0..160 {
                let storage: Vec<_> = (0..offset + size + 16)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        (state % 7) as u8
                    })
                    .collect();
                let input = &storage[offset..offset + size];
                let expected = input
                    .windows(4)
                    .position(|w| w.iter().all(|&b| b == w[0]))
                    .unwrap_or(input.len());
                assert_eq!(super::literal_prefix(input), expected);
                for start in 0..size.saturating_sub(3) {
                    let mut storage: Vec<_> =
                        (0..offset + size + 16).map(|i| (i % 251) as u8).collect();
                    storage[offset + start..offset + start + 4].fill(255);
                    assert_eq!(
                        super::literal_prefix(&storage[offset..offset + size]),
                        start
                    );
                }
            }
        }
    }

    #[test]
    fn every_pending_run_and_fragment_preserves_checked_literal_byte_state() {
        for byte in [0, 255] {
            for pending in 0..=263 {
                let (new, old) = pair();
                let (mut new, mut old) = (new, old);
                both(&mut new, &mut old, &vec![byte; pending]);
                let mut input = vec![byte; 3];
                input.extend((0..129).map(|i| (i % 251) as u8));
                input.extend([17, 17, 17, 17]);
                input.extend((0..129).rev().map(|i| (i % 251) as u8));
                input.extend(std::iter::repeat_n(byte, 260));
                let chunks = [0usize, 1, 2, 3, 4, 7, 8, 9, 31, 32, 33, 257, 259];
                let mut position = 0;
                let mut step = pending % chunks.len();
                while position < input.len() {
                    let end = (position + chunks[step % chunks.len()]).min(input.len());
                    both(&mut new, &mut old, &input[position..end]);
                    position = end;
                    step += 1;
                }
                new.flush().unwrap();
                old.flush().unwrap();
                same_admission(&new, &old);
            }
        }
    }

    #[test]
    fn long_run_count_flush_retains_the_reference_limit_plus_one_literal() {
        for limit in [8, 9, 31, 32, 33, 99999] {
            for count in [4, 5, 258] {
                let (mut new, mut old) = pair();
                new.block_limit = limit;
                old.block_limit = limit;
                let mut prefix: Vec<_> = (0..limit - 5).map(|i| (i % 251) as u8).collect();
                prefix.extend(std::iter::repeat_n(254, count));
                both(&mut new, &mut old, &prefix);
                assert_eq!(new.filling.as_ref().unwrap().input.len(), limit - 1);
                both(&mut new, &mut old, &[253]);
                assert_eq!(new.filling.as_ref().unwrap().input.len(), limit + 1);
                both(&mut new, &mut old, &[]);
                new.flush().unwrap();
                old.flush().unwrap();
                same_admission(&new, &old);
                assert_eq!(new.finish().unwrap(), old.finish().unwrap());
            }
        }
    }

    #[test]
    fn literal_repeat_cuts_at_block_limit_preserve_all_streaming_state() {
        for limit in [8, 9, 31, 32, 33, 99999] {
            for repeats in 1..=3 {
                let (mut new, mut old) = pair();
                new.block_limit = limit;
                old.block_limit = limit;
                let mut input: Vec<_> = (0..limit - repeats).map(|i| (i % 251) as u8).collect();
                input.extend(std::iter::repeat_n(254, repeats));
                both(&mut new, &mut old, &input);
                assert_eq!(new.filling.as_ref().unwrap().input.len(), limit);
                for part in [
                    &[254][..],
                    &[254, 254, 254][..],
                    &[1, 2, 3, 4, 5, 6, 7, 8, 9][..],
                ] {
                    both(&mut new, &mut old, part);
                }
                assert_eq!(new.finish().unwrap(), old.finish().unwrap());
            }
        }
    }
}
