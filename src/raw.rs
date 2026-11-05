//! Bounded incremental compression and decompression compatible with bzip2-rs.
#[cfg(feature = "compress")]
use crate::Compression;
pub use crate::compat_types::{Action, Error, Status};
use std::mem::MaybeUninit;
#[cfg(feature = "compress")]
use std::{io::Write, sync::Mutex};

/// An incremental, single-threaded BZip2 compressor using caller-owned buffers.
///
/// Compute consumed and produced lengths from changes in [`Self::total_in`] and
/// [`Self::total_out`], then advance both slices before calling again.
#[cfg(feature = "compress")]
pub struct Compress {
    // Exclusive access avoids locking; the wrapper preserves the upstream raw
    // compressor's Sync and unwind traits despite dormant worker-pool fields.
    encoder: Option<Mutex<crate::Encoder<Vec<u8>>>>,
    pending: Vec<u8>,
    position: usize,
    total_in: u64,
    total_out: u64,
    action: Option<Action>,
    remaining: usize,
    finalized: bool,
    ended: bool,
    failed: bool,
}
#[cfg(feature = "compress")]
impl Compress {
    /// Creates a compressor. The sorting engine accepts but does not use the
    /// reference implementation's `work_factor` tuning value (0 through 250).
    /// Compression uses one pass and a block size of `level * 100_000` bytes.
    ///
    /// # Panics
    ///
    /// Panics for level zero or `work_factor > 250`.
    pub fn new(level: Compression, work_factor: u32) -> Self {
        assert!(
            (1..=9).contains(&level.level()),
            "invalid compression level"
        );
        assert!(work_factor <= 250, "invalid work factor");
        Self {
            encoder: Some(Mutex::new(crate::Encoder::new(
                Vec::new(),
                crate::EncoderOptions {
                    level: level.level(),
                    block_size: Some(level.level() * 100_000),
                    threads: 1,
                    passes: Some(1),
                },
            ))),
            pending: Vec::new(),
            position: 0,
            total_in: 0,
            total_out: 0,
            action: None,
            remaining: 0,
            finalized: false,
            ended: false,
            failed: false,
        }
    }
    fn drain(&mut self, output: &mut [u8]) -> usize {
        let count = output.len().min(self.pending.len() - self.position);
        output[..count].copy_from_slice(&self.pending[self.position..self.position + count]);
        self.position += count;
        self.total_out += count as u64;
        if self.position == self.pending.len() {
            self.pending.clear();
            self.position = 0;
        }
        count
    }
    fn status(&mut self, action: Action) -> Status {
        if self.finalized && self.pending.is_empty() {
            if action == Action::Finish {
                self.ended = true;
                return Status::StreamEnd;
            }
            self.action = None;
            self.finalized = false;
            return Status::RunOk;
        }
        match action {
            Action::Run => Status::RunOk,
            Action::Flush => Status::FlushOk,
            Action::Finish => Status::FinishOk,
        }
    }
    /// Processes input and fills at most the supplied output slice.
    ///
    /// Repeat `Flush` until `RunOk`, or `Finish` until `StreamEnd`. During either
    /// operation, keep the same action and supply exactly the unconsumed suffix
    /// of its original input; a changed action or remaining length is a sequence
    /// error. Empty `Run` calls return `RunOk`, including after stream end.
    pub fn compress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        action: Action,
    ) -> Result<Status, Error> {
        // Preserve bzip2-rs's empty Run shortcut, including after stream end.
        if action == Action::Run && input.is_empty() {
            return Ok(Status::RunOk);
        }
        if self.ended || self.failed {
            return Err(Error::Sequence);
        }
        if let Some(previous) = self.action {
            if previous != action || input.len() != self.remaining {
                return Err(Error::Sequence);
            }
        } else if action != Action::Run {
            self.action = Some(action);
            self.remaining = input.len();
        }
        let written = self.drain(output);
        if !self.pending.is_empty() || self.finalized {
            return Ok(self.status(action));
        }
        let amount = input.len().min(16 * 1024);
        let encoder = self
            .encoder
            .as_mut()
            .ok_or(Error::Sequence)?
            .get_mut()
            .expect("raw encoder mutex is never locked");
        if amount != 0 {
            if encoder.write_all(&input[..amount]).is_err() {
                self.failed = true;
                return Err(Error::Data);
            }
            self.total_in += amount as u64;
            if action != Action::Run {
                self.remaining -= amount;
            }
        }
        if action != Action::Run && self.remaining == 0 {
            if action == Action::Flush {
                if encoder.flush_block().is_err() {
                    self.failed = true;
                    return Err(Error::Data);
                }
                self.pending = std::mem::take(encoder.get_mut());
            } else {
                match self
                    .encoder
                    .take()
                    .expect("active encoder")
                    .into_inner()
                    .expect("raw encoder mutex is never locked")
                    .finish()
                {
                    Ok(bytes) => self.pending = bytes,
                    Err(_) => {
                        self.failed = true;
                        return Err(Error::Data);
                    }
                }
            }
            self.finalized = true;
        } else {
            self.pending = std::mem::take(encoder.get_mut());
        }
        self.drain(&mut output[written..]);
        Ok(self.status(action))
    }
    /// Processes input into uninitialized storage; only produced bytes are written.
    ///
    /// A temporary buffer limits each call to at most 65,536 output bytes. Only
    /// the prefix measured by the output-counter delta becomes initialized.
    pub fn compress_uninit(
        &mut self,
        input: &[u8],
        output: &mut [MaybeUninit<u8>],
        action: Action,
    ) -> Result<Status, Error> {
        let mut scratch = vec![0; output.len().min(65536)];
        let before = self.total_out;
        let result = self.compress(input, &mut scratch, action);
        let produced = (self.total_out - before) as usize;
        for (target, byte) in output.iter_mut().zip(&scratch[..produced]) {
            target.write(*byte);
        }
        result
    }
    /// Appends to spare vector capacity without growing the supplied allocation.
    ///
    /// The vector must have spare capacity to receive output. A temporary buffer
    /// limits each call to at most 65,536 output bytes; existing elements remain.
    pub fn compress_vec(
        &mut self,
        input: &[u8],
        output: &mut Vec<u8>,
        action: Action,
    ) -> Result<Status, Error> {
        let mut scratch = vec![0; (output.capacity() - output.len()).min(65536)];
        let before = self.total_out;
        let result = self.compress(input, &mut scratch, action);
        output.extend_from_slice(&scratch[..(self.total_out - before) as usize]);
        result
    }
    /// Number of input bytes accepted.
    pub fn total_in(&self) -> u64 {
        self.total_in
    }
    /// Number of output bytes delivered.
    pub fn total_out(&self) -> u64 {
        self.total_out
    }
}

/// An incremental single-stream BZip2 decompressor using caller-owned buffers.
///
/// Exhausting an input slice is not end-of-stream: provide more input until
/// [`Status::StreamEnd`] validates the trailer. Stop at that status; subsequent
/// calls return [`Error::Sequence`]. Bytes after the first stream remain unconsumed.
///
/// ```
/// use bzip2_turbo::{Decompress, Status};
/// // A complete, empty BZip2 stream.
/// let encoded = b"BZh9\x17\x72\x45\x38\x50\x90\0\0\0\0";
/// let mut decoder = Decompress::new(true);
/// let mut input = encoded.as_slice();
/// let mut output = [0; 16];
/// loop {
///     let before = decoder.total_in();
///     let status = decoder.decompress(input, &mut output)?;
///     input = &input[(decoder.total_in() - before) as usize..];
///     if status == Status::StreamEnd { break; }
///     assert!(!input.is_empty(), "truncated stream");
/// }
/// assert_eq!(decoder.total_out(), 0);
/// # Ok::<(), bzip2_turbo::Error>(())
/// ```
pub struct Decompress {
    decoder: crate::decoder::Incremental,
    total_in: u64,
    total_out: u64,
    failed: bool,
    ended: bool,
}
impl Decompress {
    /// Creates a decompressor. `small` selects compact 32-bit reconstruction
    /// storage; this differs from libbz2's slower 16-bit small-memory algorithm.
    /// `false` uses the platform default, paired words only on macOS ARM64.
    /// No worker thread is started.
    pub fn new(small: bool) -> Self {
        Self {
            decoder: crate::decoder::Incremental::new(small),
            total_in: 0,
            total_out: 0,
            failed: false,
            ended: false,
        }
    }
    /// Processes input and fills at most the supplied output slice.
    ///
    /// Counter deltas identify the consumed and produced prefixes, including
    /// progress before an error. `Ok` may mean more input or output is needed;
    /// continue draining pending output even after input is exhausted. If no
    /// progress remains possible at input EOF before `StreamEnd`, the caller
    /// must report truncation.
    /// Malformed input produces `Data` or `DataMagic`; errors poison the decoder.
    pub fn decompress(&mut self, input: &[u8], output: &mut [u8]) -> Result<Status, Error> {
        if self.failed || self.ended {
            return Err(Error::Sequence);
        }
        let mut remaining = input;
        let mut written = 0;
        loop {
            let before = remaining.len();
            let result = self.decoder.process(&mut remaining, &mut output[written..]);
            self.total_in += (before - remaining.len()) as u64;
            self.total_out = self.decoder.delivered();
            match result {
                Ok(count) => {
                    written += count;
                }
                Err(error) => {
                    self.failed = true;
                    let magic = error.to_string().contains("header")
                        || error.to_string().contains("block size");
                    return Err(if magic { Error::DataMagic } else { Error::Data });
                }
            }
            if self.decoder.done() {
                self.ended = true;
                return Ok(Status::StreamEnd);
            }
            if written == output.len() || (before == remaining.len() && written == 0) {
                return Ok(Status::Ok);
            }
            // After draining a block, process the footer even when no output was
            // produced, but stop once an input-starved parser makes no progress.
            if before == remaining.len() && remaining.is_empty() {
                return Ok(Status::Ok);
            }
        }
    }
    /// Processes input into uninitialized storage; only produced bytes are written.
    ///
    /// A temporary buffer limits each call to at most 65,536 output bytes. Only
    /// the prefix measured by the output-counter delta becomes initialized.
    pub fn decompress_uninit(
        &mut self,
        input: &[u8],
        output: &mut [MaybeUninit<u8>],
    ) -> Result<Status, Error> {
        let mut scratch = vec![0; output.len().min(65536)];
        let before = self.total_out;
        let result = self.decompress(input, &mut scratch);
        let produced = (self.total_out - before) as usize;
        for (target, byte) in output.iter_mut().zip(&scratch[..produced]) {
            target.write(*byte);
        }
        result
    }
    /// Appends to spare vector capacity without growing the supplied allocation.
    ///
    /// The vector must have spare capacity to receive output. A temporary buffer
    /// limits each call to at most 65,536 output bytes; existing elements remain.
    pub fn decompress_vec(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<Status, Error> {
        let mut scratch = vec![0; (output.capacity() - output.len()).min(65536)];
        let before = self.total_out;
        let result = self.decompress(input, &mut scratch);
        output.extend_from_slice(&scratch[..(self.total_out - before) as usize]);
        result
    }
    /// Number of compressed bytes accepted.
    pub fn total_in(&self) -> u64 {
        self.total_in
    }
    /// Number of decoded bytes delivered.
    pub fn total_out(&self) -> u64 {
        self.total_out
    }
}
