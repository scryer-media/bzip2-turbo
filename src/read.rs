//! Compatibility adapters that transform bytes read from an underlying [`Read`].
//!
//! These adapters add a [`BufReader`]. Recovering the reader discards unread
//! buffered bytes; use [`crate::bufread`] when trailing input must remain accessible.
//!
//! When an adapter also implements [`Write`], writes are forwarded directly to
//! the underlying stream without compression or decompression. Use
//! [`crate::write`] to transform bytes supplied through `Write`.

// Adapted from bzip2 0.6.1; see LICENSE-BZIP2-MIT for its license.

use std::io::prelude::*;
use std::io::{self, BufReader};

#[cfg(feature = "compress")]
use crate::Compression;
use crate::bufread;

/// A compression stream which wraps an uncompressed stream of data. Compressed
/// data will be read from the stream.
#[cfg(feature = "compress")]
pub struct BzEncoder<R> {
    inner: bufread::BzEncoder<BufReader<R>>,
}

/// A decompression stream which wraps a compressed stream of data. Decompressed
/// data will be read from the stream. Decoding stops after the first BZip2
/// member; use [`MultiBzDecoder`] for concatenated members.
pub struct BzDecoder<R> {
    inner: bufread::BzDecoder<BufReader<R>>,
}

#[cfg(feature = "compress")]
impl<R: Read> BzEncoder<R> {
    /// Creates a reader that compresses input at the given compatibility level.
    pub fn new(r: R, level: Compression) -> Self {
        Self {
            inner: bufread::BzEncoder::new(BufReader::new(r), level),
        }
    }

    /// Acquires a reference to the underlying stream
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying stream
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// the adapter continues to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut().get_mut()
    }

    /// Returns the underlying reader without completing or validating the stream.
    /// Buffered input and pending codec output are discarded.
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }

    /// Number of output bytes produced by the codec.
    ///
    /// Buffered output may not yet have reached the caller or underlying writer.
    pub fn total_out(&self) -> u64 {
        self.inner.total_out()
    }

    /// Number of input bytes consumed by the codec.
    ///
    /// This excludes any bytes read ahead but not yet consumed by the codec.
    pub fn total_in(&self) -> u64 {
        self.inner.total_in()
    }
}

#[cfg(feature = "compress")]
impl<R: Read> Read for BzEncoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

#[cfg(feature = "compress")]
impl<W: Write + Read> Write for BzEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

impl<R: Read> BzDecoder<R> {
    /// Create a new decompression stream, which will read compressed
    /// data from the given input stream and decompress it.
    pub fn new(r: R) -> Self {
        Self {
            inner: bufread::BzDecoder::new(BufReader::new(r)),
        }
    }

    /// Acquires a reference to the underlying stream
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying stream
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// the adapter continues to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut().get_mut()
    }

    /// Returns the underlying reader without completing or validating the stream.
    /// Buffered input and pending codec output are discarded.
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }

    /// Number of output bytes produced by the codec.
    ///
    /// Buffered output may not yet have reached the caller or underlying writer.
    pub fn total_out(&self) -> u64 {
        self.inner.total_out()
    }

    /// Number of input bytes consumed by the codec.
    ///
    /// This excludes any bytes read ahead but not yet consumed by the codec.
    pub fn total_in(&self) -> u64 {
        self.inner.total_in()
    }
}

impl<R: Read> Read for BzDecoder<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.inner.read(into)
    }
}

impl<W: Write + Read> Write for BzDecoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

/// A bzip2 streaming decoder that decodes all members of a multistream
///
/// Wikipedia, particularly, uses bzip2 multistream for their dumps.
pub struct MultiBzDecoder<R> {
    inner: bufread::MultiBzDecoder<BufReader<R>>,
}

impl<R: Read> MultiBzDecoder<R> {
    /// Creates a decoder for concatenated BZip2 streams.
    /// Input is parsed lazily when read; all members are decoded.
    pub fn new(r: R) -> Self {
        Self {
            inner: bufread::MultiBzDecoder::new(BufReader::new(r)),
        }
    }
}

impl<R> MultiBzDecoder<R> {
    /// Acquires a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying stream.
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// the adapter continues to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut().get_mut()
    }

    /// Returns the underlying reader, discarding buffered input without validating
    /// any unread stream trailer.
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }
}

impl<R: Read> Read for MultiBzDecoder<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.inner.read(into)
    }
}
