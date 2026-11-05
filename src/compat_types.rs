// API definitions adapted from bzip2-rs, MIT OR Apache-2.0.
use std::{error, fmt};

/// Operation requested from the incremental compressor.
#[derive(PartialEq, Eq, Copy, Debug, Clone)]
pub enum Action {
    /// Accept input and emit available compressed output.
    Run = 0,
    /// Consume the supplied input and finish the current block without ending the stream.
    Flush = 1,
    /// Request that the compression stream be finalized.
    Finish = 2,
}

/// Progress reported by the incremental compatibility APIs.
///
/// Use the cumulative input/output counters to determine how many bytes a call
/// consumed and produced. A successful status does not imply either buffer was filled.
#[derive(PartialEq, Eq, Copy, Debug, Clone)]
pub enum Status {
    /// Decompression needs more input or output space to continue.
    Ok,

    /// A compression flush is still in progress.
    FlushOk,

    /// Compression can accept another run operation; a requested flush may be complete.
    RunOk,

    /// Compression is finishing; continue until `StreamEnd`.
    FinishOk,

    /// The stream trailer has been completed or validated.
    StreamEnd,

    /// Reserved for compatibility; this implementation does not return this status.
    MemNeeded,
}

/// Errors returned by incremental compression or decompression.
///
/// Counters may have advanced before a data/codec failure, which poisons the
/// codec. A rejected compression action sequence does not itself poison it;
/// the caller can resume the pending operation with its correct action/input.
#[derive(PartialEq, Eq, Copy, Debug, Clone)]
pub enum Error {
    /// The sequence of operations called on a decompression/compression stream
    /// were invalid. See methods for details.
    Sequence,

    /// The data being decompressed was invalid, or it was not a valid bz2
    /// stream.
    Data,

    /// The magic bz2 header wasn't present when decompressing.
    DataMagic,

    /// The parameters to this function were invalid.
    Param,
}

/// Compression level for the compatibility adapters, normally `1..=9`.
///
/// Native compression uses `EncoderOptions` when `compress` is enabled.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Compression(u32);

impl Compression {
    /// Create a new compression spec with a specific numeric level in the range `1..=9`.
    ///
    /// # Panics
    ///
    /// A level outside of the `1..=9` range panics. Use [`Self::try_new`] to
    /// gracefully handle invalid levels (e.g. from user input).
    #[track_caller]
    pub const fn new(level: u32) -> Self {
        match Self::try_new(level) {
            Some(v) => v,
            None => panic!("expected a compression level in the range 1..=9"),
        }
    }

    /// Create a new compression spec with a specific numeric level in the range `1..=9`.
    pub const fn try_new(level: u32) -> Option<Self> {
        match level {
            1..=9 => Some(Self(level)),
            _ => None,
        }
    }

    /// Returns the legacy invalid level zero.
    ///
    /// Compressor constructors reject this value; BZip2 has no stored-data level.
    #[deprecated(since = "0.5.1", note = "libbz2 does not support compression level 0")]
    pub fn none() -> Self {
        Self(0)
    }

    /// Selects level one and a 100,000-byte compatibility block size.
    pub const fn fast() -> Self {
        Self(1)
    }

    /// Selects level nine and a 900,000-byte compatibility block size.
    pub const fn best() -> Self {
        Self(9)
    }

    /// Return the compression level as an integer.
    pub const fn level(&self) -> u32 {
        self.0
    }
}

impl Default for Compression {
    /// Choose the default compression, a balance between speed and size.
    fn default() -> Self {
        Self(6)
    }
}

impl error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let description = match self {
            Self::Sequence => "bzip2: sequence of operations invalid",
            Self::Data => "bzip2: invalid data",
            Self::DataMagic => "bzip2: bz2 header missing",
            Self::Param => "bzip2: invalid parameters",
        };
        f.write_str(description)
    }
}

impl From<Error> for std::io::Error {
    fn from(data: Error) -> Self {
        Self::other(data)
    }
}
