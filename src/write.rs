//! Compatibility [`Write`] adapters for compression and single-stream decompression.
//!
//! Call `finish` or `try_finish` explicitly to report final codec and output
//! errors. Drop attempts completion but cannot return errors. Counters describe
//! codec progress, which can precede writes to the underlying writer.

// Adapted from bzip2 0.6.1; see LICENSE-BZIP2-MIT for its license.

use std::io;
use std::io::prelude::*;

#[cfg(feature = "compress")]
use crate::{Action, Compress, Compression};
use crate::{Decompress, Status};

/// A compression stream which will have uncompressed data written to it and
/// will write compressed data to an output stream.
///
/// ```
/// use bzip2_turbo::{write::BzEncoder, read::BzDecoder, Compression};
/// use std::io::{Read, Write};
/// let mut encoder = BzEncoder::new(Vec::new(), Compression::fast());
/// encoder.write_all(b"compatibility stream")?;
/// let encoded = encoder.finish()?;
/// let mut decoded = Vec::new();
/// BzDecoder::new(encoded.as_slice()).read_to_end(&mut decoded)?;
/// assert_eq!(decoded, b"compatibility stream");
/// # Ok::<(), std::io::Error>(())
/// ```
#[cfg(feature = "compress")]
pub struct BzEncoder<W: Write> {
    data: Compress,
    obj: Option<W>,
    buf: Vec<u8>,
    done: bool,
    panicked: bool,
}

/// A decompression stream which will have compressed data written to it and
/// will write uncompressed data to an output stream.
pub struct BzDecoder<W: Write> {
    data: Decompress,
    obj: Option<W>,
    buf: Vec<u8>,
    done: bool,
    panicked: bool,
}

#[cfg(feature = "compress")]
impl<W: Write> BzEncoder<W> {
    /// Creates a writer that compresses bytes at the given compatibility level.
    pub fn new(obj: W, level: Compression) -> Self {
        Self {
            data: Compress::new(level, 30),
            obj: Some(obj),
            buf: Vec::with_capacity(32 * 1024),
            done: false,
            panicked: false,
        }
    }

    fn dump(&mut self) -> io::Result<()> {
        while !self.buf.is_empty() {
            self.panicked = true;
            let r = self.obj.as_mut().unwrap().write(&self.buf);
            self.panicked = false;

            match r {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => self.buf.drain(..n),
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            };
        }
        Ok(())
    }

    /// Acquires a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        self.obj.as_ref().unwrap()
    }

    /// Acquires a mutable reference to the underlying writer.
    ///
    /// Note that mutating the output/input state of the stream may corrupt this
    /// object, so care must be taken when using this method.
    pub fn get_mut(&mut self) -> &mut W {
        self.obj.as_mut().unwrap()
    }

    /// Attempt to finish this output stream, writing out final chunks of data.
    ///
    /// Note that this function can only be used once data has finished being
    /// written to the output stream. After this function is called then further
    /// calls to [`write`] may result in a panic.
    ///
    /// # Panics
    ///
    /// Attempts to write data to this stream may result in a panic after this
    /// function is called.
    ///
    /// [`write`]: Self::write
    pub fn try_finish(&mut self) -> io::Result<()> {
        while !self.done {
            self.dump()?;
            let res = self.data.compress_vec(&[], &mut self.buf, Action::Finish);
            if res == Ok(Status::StreamEnd) {
                self.done = true;
                break;
            }
        }
        self.dump()
    }

    /// Completes the BZip2 stream and returns its writer.
    ///
    /// Pending compressed bytes are written, but the underlying writer is not
    /// flushed. Call its [`Write::flush`] if required. This is blocking I/O;
    /// [`Self::try_finish`] allows completion before consuming the adapter.
    pub fn finish(mut self) -> io::Result<W> {
        self.try_finish()?;
        Ok(self.obj.take().unwrap())
    }

    /// Number of output bytes produced by the codec.
    ///
    /// Buffered output may not yet have reached the caller or underlying writer.
    pub fn total_out(&self) -> u64 {
        self.data.total_out()
    }

    /// Number of input bytes consumed by the codec.
    ///
    /// This excludes any bytes read ahead but not yet consumed by the codec.
    pub fn total_in(&self) -> u64 {
        self.data.total_in()
    }
}

#[cfg(feature = "compress")]
impl<W: Write> Write for BzEncoder<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        loop {
            self.dump()?;

            let total_in = self.total_in();
            self.data
                .compress_vec(data, &mut self.buf, Action::Run)
                .unwrap();
            let written = (self.total_in() - total_in) as usize;

            if written > 0 || data.is_empty() {
                return Ok(written);
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        loop {
            self.dump()?;
            let before = self.total_out();
            self.data
                .compress_vec(&[], &mut self.buf, Action::Flush)
                .unwrap();

            if before == self.total_out() {
                break;
            }
        }
        self.obj.as_mut().unwrap().flush()
    }
}

impl<W: Write> BzDecoder<W> {
    /// Create a new decoding stream which will decompress all data written
    /// to it into `obj`.
    pub fn new(obj: W) -> Self {
        Self {
            data: Decompress::new(false),
            obj: Some(obj),
            buf: Vec::with_capacity(32 * 1024),
            done: false,
            panicked: false,
        }
    }

    /// Acquires a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        self.obj.as_ref().unwrap()
    }

    /// Acquires a mutable reference to the underlying writer.
    ///
    /// Note that mutating the output/input state of the stream may corrupt this
    /// object, so care must be taken when using this method.
    pub fn get_mut(&mut self) -> &mut W {
        self.obj.as_mut().unwrap()
    }

    fn dump(&mut self) -> io::Result<()> {
        while !self.buf.is_empty() {
            self.panicked = true;
            let r = self.obj.as_mut().unwrap().write(&self.buf);
            self.panicked = false;

            match r {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => self.buf.drain(..n),
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            };
        }
        Ok(())
    }

    /// Drains pending output and requires a complete, validated BZip2 stream.
    ///
    /// Returns [`io::ErrorKind::UnexpectedEof`] if the supplied compressed input
    /// ended before the stream trailer. This does not flush the underlying writer.
    pub fn try_finish(&mut self) -> io::Result<()> {
        while !self.done {
            // The write is effectively a `self.flush()`, but we want to know how many
            // bytes were written. exit if no input was read and no output was written
            if self.write(&[])? == 0 {
                // finishing the output stream is effectively EOF of the input
                let msg = "Input EOF reached before logical stream end";
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, msg));
            }
        }
        self.dump()
    }

    /// Validates stream completion, writes pending bytes and takes the writer.
    ///
    /// This does not flush the underlying writer.
    ///
    /// # Panics
    ///
    /// Panics if the writer was already taken by a successful call to this method.
    /// Do not use the adapter after taking its writer.
    pub fn finish(&mut self) -> io::Result<W> {
        self.try_finish()?;
        Ok(self.obj.take().unwrap())
    }

    /// Number of output bytes produced by the codec.
    ///
    /// Buffered output may not yet have reached the caller or underlying writer.
    pub fn total_out(&self) -> u64 {
        self.data.total_out()
    }

    /// Number of input bytes consumed by the codec.
    ///
    /// This excludes any bytes read ahead but not yet consumed by the codec.
    pub fn total_in(&self) -> u64 {
        self.data.total_in()
    }
}

impl<W: Write> Write for BzDecoder<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        loop {
            self.dump()?;

            let before = self.total_in();
            let res = self.data.decompress_vec(data, &mut self.buf);
            let written = (self.total_in() - before) as usize;

            // make sure that a subsequent call exits early when there is nothing useful left to do
            self.done |= matches!(res, Err(_) | Ok(Status::StreamEnd));

            if let Err(e) = res {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, e));
            }

            if written > 0 || data.is_empty() || self.done {
                return Ok(written);
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.dump()?;
        self.obj.as_mut().unwrap().flush()
    }
}

impl<W: Write> Drop for BzDecoder<W> {
    fn drop(&mut self) {
        if self.obj.is_some() {
            let _ = self.try_finish();
        }
    }
}

#[cfg(feature = "compress")]
impl<W: Write> Drop for BzEncoder<W> {
    fn drop(&mut self) {
        if self.obj.is_some() && !self.panicked {
            let _ = self.try_finish();
        }
    }
}
