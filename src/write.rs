//! Writer-based compression/decompression streams

// Adapted from bzip2 0.6.1; see LICENSE-BZIP2-MIT for its license.

use std::io;
use std::io::prelude::*;

#[cfg(feature = "compress")]
use crate::{Action, Compress, Compression};
use crate::{Decompress, Status};

/// A compression stream which will have uncompressed data written to it and
/// will write compressed data to an output stream.
#[cfg(feature = "compress")]
pub struct BzEncoder<W: Write> {
    data: Compress,
    obj: Option<W>,
    buf: Vec<u8>,
    done: bool,
    panicked: bool,
}

/// A compression stream which will have compressed data written to it and
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
    /// Create a new compression stream which will compress at the given level
    /// to write compress output to the give output stream.
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

    /// Consumes this encoder, flushing the output stream.
    ///
    /// This will flush the underlying data stream and then return the contained
    /// writer if the flush succeeded.
    ///
    /// Note that this function may not be suitable to call in a situation where
    /// the underlying stream is an asynchronous I/O stream. To finish a stream
    /// the [`try_finish`] (or `shutdown`) method should be used instead. To
    /// re-acquire ownership of a stream it is safe to call this method after
    /// [`try_finish`] or `shutdown` has returned `Ok`.
    ///
    /// [`try_finish`]: Self::try_finish
    pub fn finish(mut self) -> io::Result<W> {
        self.try_finish()?;
        Ok(self.obj.take().unwrap())
    }

    /// Returns the number of bytes produced by the compressor
    ///
    /// Note that, due to buffering, this only bears any relation to
    /// [`total_in`] after a call to [`flush`].  At that point,
    /// `total_out() / total_in()` is the compression ratio.
    ///
    /// [`flush`]: Self::flush
    /// [`total_in`]: Self::total_in
    pub fn total_out(&self) -> u64 {
        self.data.total_out()
    }

    /// Returns the number of bytes consumed by the compressor
    /// (e.g. the number of bytes written to this stream.)
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

    /// Unwrap the underlying writer, finishing the compression stream.
    ///
    /// Note that this function may not be suitable to call in a situation where
    /// the underlying stream is an asynchronous I/O stream. To finish a stream
    /// the [`try_finish`] (or `shutdown`) method should be used instead. To
    /// re-acquire ownership of a stream it is safe to call this method after
    /// [`try_finish`] or `shutdown` has returned `Ok`.
    ///
    /// [`try_finish`]: Self::try_finish
    pub fn finish(&mut self) -> io::Result<W> {
        self.try_finish()?;
        Ok(self.obj.take().unwrap())
    }

    /// Returns the number of bytes produced by the decompressor
    ///
    /// Note that, due to buffering, this only bears any relation to
    /// [`total_in`] after a call to [`flush`].  At that point,
    /// `total_in() / total_out()` is the compression ratio.
    ///
    /// [`flush`]: Self::flush
    /// [`total_in`]: Self::total_in
    pub fn total_out(&self) -> u64 {
        self.data.total_out()
    }

    /// Returns the number of bytes consumed by the decompressor
    /// (e.g. the number of bytes written to this stream.)
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
