//! Compatibility [`Read`] adapters over caller-supplied [`BufRead`] input.
//!
//! The codec consumes only parsed input through [`BufRead::consume`]. A single
//! [`BzDecoder`] stops at its first member, leaving following bytes in the
//! underlying buffered reader; [`MultiBzDecoder`] continues through all members.
//!
//! When an adapter also implements [`Write`], writes are forwarded directly to
//! the underlying stream without compression or decompression. Use
//! [`crate::write`] to transform bytes supplied through `Write`.

// Adapted from bzip2 0.6.1; see LICENSE-BZIP2-MIT for its license.

use std::io;
use std::io::prelude::*;

#[cfg(feature = "compress")]
use crate::{Action, Compress, Compression};
use crate::{Decompress, Status};

/// A bz2 encoder, or compressor.
///
/// This structure implements a [`Read`] interface and will read uncompressed
/// data from an underlying stream and emit a stream of compressed data.
#[cfg(feature = "compress")]
pub struct BzEncoder<R> {
    obj: R,
    data: Compress,
    done: bool,
}

/// A bz2 decoder, or decompressor.
///
/// This structure implements a [`Read`] interface and takes a stream of
/// compressed data as input, providing the decompressed data when read from.
pub struct BzDecoder<R> {
    obj: R,
    data: Decompress,
    done: bool,
    multi: bool,
}

#[cfg(feature = "compress")]
impl<R: BufRead> BzEncoder<R> {
    /// Creates a new encoder which will read uncompressed data from the given
    /// stream and emit the compressed stream.
    pub fn new(r: R, level: Compression) -> Self {
        Self {
            obj: r,
            data: Compress::new(level, 30),
            done: false,
        }
    }
}

#[cfg(feature = "compress")]
impl<R> BzEncoder<R> {
    /// Acquires a reference to the underlying stream
    pub fn get_ref(&self) -> &R {
        &self.obj
    }

    /// Acquires a mutable reference to the underlying stream
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// the adapter continues to be used.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.obj
    }

    /// Returns the underlying reader without finishing compression.
    /// Pending compressed output is discarded.
    pub fn into_inner(self) -> R {
        self.obj
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
impl<R: BufRead> Read for BzEncoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        loop {
            let (read, consumed, eof, ret);
            {
                let input = self.obj.fill_buf()?;
                eof = input.is_empty();
                let before_out = self.data.total_out();
                let before_in = self.data.total_in();
                let action = if eof { Action::Finish } else { Action::Run };
                ret = self.data.compress(input, buf, action);
                read = (self.data.total_out() - before_out) as usize;
                consumed = (self.data.total_in() - before_in) as usize;
            }
            self.obj.consume(consumed);

            // we should never get the sequence error that's possible to be
            // returned from compression
            let ret = ret.unwrap();

            // If we haven't ready any data and we haven't hit EOF yet, then we
            // need to keep asking for more data because if we return that 0
            // bytes of data have been read then it will be interpreted as EOF.
            if read == 0 && !eof && !buf.is_empty() {
                continue;
            }
            if ret == Status::StreamEnd {
                self.done = true;
            }
            return Ok(read);
        }
    }
}

#[cfg(feature = "compress")]
impl<W: Write> Write for BzEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

impl<R: BufRead> BzDecoder<R> {
    /// Creates a new decoder which will decompress data read from the given
    /// stream.
    pub fn new(r: R) -> Self {
        Self {
            obj: r,
            data: Decompress::new(false),
            done: false,
            multi: false,
        }
    }

    fn multi(mut self, flag: bool) -> Self {
        self.multi = flag;
        self
    }
}

impl<R> BzDecoder<R> {
    /// Acquires a reference to the underlying stream
    pub fn get_ref(&self) -> &R {
        &self.obj
    }

    /// Acquires a mutable reference to the underlying stream
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// the adapter continues to be used.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.obj
    }

    /// Returns the underlying reader without validating any unread trailer.
    /// Buffered bytes not consumed by the codec remain in that reader.
    pub fn into_inner(self) -> R {
        self.obj
    }

    /// Number of input bytes consumed by the codec.
    ///
    /// This excludes any bytes read ahead but not yet consumed by the codec.
    pub fn total_in(&self) -> u64 {
        self.data.total_in()
    }

    /// Number of output bytes produced by the codec.
    ///
    /// Buffered output may not yet have reached the caller or underlying writer.
    pub fn total_out(&self) -> u64 {
        self.data.total_out()
    }
}

impl<R: BufRead> Read for BzDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.done && !self.multi {
                return Ok(0);
            }
            let (read, consumed, remaining, ret);
            {
                let input = self.obj.fill_buf()?;
                if self.done {
                    assert!(self.multi);
                    if input.is_empty() {
                        // beyond last stream in multi-stream case
                        return Ok(0);
                    } else {
                        // previous stream ended, more data follows => create new decompressor
                        self.data = Decompress::new(false);
                        self.done = false;
                    }
                }
                let before_out = self.data.total_out();
                let before_in = self.data.total_in();
                ret = self.data.decompress(input, buf);
                read = (self.data.total_out() - before_out) as usize;
                consumed = (self.data.total_in() - before_in) as usize;
                remaining = input.len() - consumed;
            }
            self.obj.consume(consumed);

            let ret = ret.map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            if ret == Status::StreamEnd {
                self.done = true;
            } else if consumed == 0 && remaining == 0 && read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "decompression not finished but EOF reached",
                ));
            }

            if read > 0 || buf.is_empty() {
                return Ok(read);
            }
        }
    }
}

impl<W: Write> Write for BzDecoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

/// A bzip2 streaming decoder that decodes all members of a multistream.
///
/// Wikipedia, particularly, uses bzip2 multistream for their dumps, and the
/// `pbzip2` tool creates such data as well.
pub struct MultiBzDecoder<R>(BzDecoder<R>);

impl<R: BufRead> MultiBzDecoder<R> {
    /// Creates a new decoder from the given reader. If the bzip2 stream contains multiple members
    /// all will be decoded.
    pub fn new(r: R) -> Self {
        Self(BzDecoder::new(r).multi(true))
    }
}

impl<R> MultiBzDecoder<R> {
    /// Acquires a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        self.0.get_ref()
    }

    /// Acquires a mutable reference to the underlying stream.
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// the adapter continues to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.0.get_mut()
    }

    /// Returns the underlying reader without validating any unread trailer.
    /// Buffered bytes not consumed by the codec remain in that reader.
    pub fn into_inner(self) -> R {
        self.0.into_inner()
    }
}

impl<R: BufRead> Read for MultiBzDecoder<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.0.read(into)
    }
}

impl<R: BufRead + Write> Write for MultiBzDecoder<R> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}
