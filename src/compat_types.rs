// API definitions adapted from bzip2-rs, MIT OR Apache-2.0.
use std::{error, fmt};

/// Possible actions to take on compression.
#[derive(PartialEq, Eq, Copy, Debug, Clone)]
pub enum Action {
    /// Normal compression.
    Run = 0,
    /// Flush any existing output, but do not read any more input
    Flush = 1,
    /// Request that the compression stream be finalized.
    Finish = 2,
}

/// Result of compression or decompression
#[derive(PartialEq, Eq, Copy, Debug, Clone)]
pub enum Status {
    /// Decompression went fine, nothing much to report.
    Ok,

    /// The Flush action on a compression went ok.
    FlushOk,

    /// The Run action on compression went ok.
    RunOk,

    /// The Finish action on compression went ok.
    FinishOk,

    /// The stream's end has been met, meaning that no more data can be input.
    StreamEnd,

    /// There was insufficient memory in the input or output buffer to complete
    /// the request, but otherwise everything went normally.
    MemNeeded,
}

/// Fatal errors encountered when compressing/decompressing bytes.
///
/// These errors indicate that progress could not be made in any form due to
/// input or output parameters.
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

/// When compressing data, the compression level can be specified by a value in
/// this enum.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Compression(u32);

impl Compression {
    /// Create a new compression spec with a specific numeric level in the range `1..=9`.
    ///
    /// # Panics
    ///
    /// A level outside of the `1..=9` range will throw a panic. Use [`Self::try_new`] to
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

    /// Do not compress.
    #[deprecated(since = "0.5.1", note = "libbz2 does not support compression level 0")]
    pub fn none() -> Self {
        Self(0)
    }

    /// Optimize for the best speed of encoding.
    pub const fn fast() -> Self {
        Self(1)
    }

    /// Optimize for smallest output size.
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
