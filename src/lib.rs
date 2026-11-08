//! Streaming BZip2 compression and decompression.
//!
//! [`Decoder`] is the native reader API. With the default `compress` feature,
//! `Encoder` and `EncoderOptions` provide native streaming compression.
//! The [`read`], [`bufread`] and [`mod@write`] modules provide compatibility adapters;
//! [`Decompress`] and, with `compress`, `Compress` expose caller-owned buffers.
//!
//! Native decoding accepts concatenated streams by default. The compatibility
//! `BzDecoder` adapters stop at one stream; use their `MultiBzDecoder` reader
//! adapters for concatenated streams. Native encoder settings follow the 7-Zip
//! codec; compatibility compression levels use their own block-size mapping.
//!
//! # Features
//!
//! `compress` enables compression and is enabled by default. Disabling default
//! features retains decompression and its adapters. This crate uses standard I/O
//! and is not a `no_std` library.
//!
//! # Example
//!
//! ```
//! # #[cfg(feature = "compress")] {
//! use bzip2_turbo::{Decoder, Encoder, EncoderOptions};
//! use std::io::{Read, Write};
//!
//! let mut encoder = Encoder::new(Vec::new(), EncoderOptions::default());
//! encoder.write_all(b"a streaming example")?;
//! let compressed = encoder.finish()?;
//! let mut restored = Vec::new();
//! Decoder::new(compressed.as_slice()).read_to_end(&mut restored)?;
//! assert_eq!(restored, b"a streaming example");
//! # }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Call `Encoder::finish` to write the stream trailer and report final I/O errors;
//! dropping a native encoder does not finish its stream. Read a decoder through
//! EOF to validate the complete stream, including its trailer checksum.
//!
//! # Differences from the `bzip2` crate
//!
//! The adapters follow bzip2 0.6.1's interface and stream behavior; the backend
//! differs:
//!
//! - Compressed bytes can differ from libbz2. Matching native settings
//!   reproduce 7-Zip's bytes.
//! - Compatibility [`Compression`] levels select 100,000-byte block increments
//!   with a default of six; native `EncoderOptions` keeps 7-Zip's level and
//!   pass mapping with a default of five, except that level nine runs one
//!   pass where 7-Zip runs seven.
//! - `work_factor` is validated in `0..=250` but does not tune the 7-Zip
//!   sorter.
//! - Compact decoding (`Decompress::new(true)`, [`Decoder::with_low_memory`])
//!   uses 32-bit links, not libbz2's 16-bit small-memory representation, and
//!   does not promise its footprint. Memory estimates cover codec allocations,
//!   not whole-process RSS.
//! - Low-level calls can consume or produce less than their slice capacity;
//!   the compatibility bridge admits at most 16 KiB of input per call.
//! - A zero-progress underlying writer returns `WriteZero` instead of looping.
//! - The original package's backend and linkage Cargo features do not exist.
//!
//! The codec is licensed under LGPL-2.1-or-later; compatibility adapters retain MIT.
//! The repository's `docs/validation.md` records reference revisions and
//! verification evidence.

#![deny(unsafe_code)]
#![cfg_attr(feature = "compress", doc = include_str!("../README.md"))]

mod compat_types;
#[forbid(unsafe_code)]
mod decoder;
#[cfg(feature = "compress")]
#[forbid(unsafe_code)]
mod encoder;
mod raw;
mod tt_storage;

pub mod bufread;
pub mod read;
pub mod write;

pub use compat_types::Compression;
#[cfg(feature = "compress")]
pub use raw::Compress;
pub use raw::{Action, Decompress, Error, Status};

pub use decoder::Decoder;
#[cfg(feature = "compress")]
pub use encoder::{Encoder, EncoderOptions};

#[inline]
pub(crate) fn update_crc(crc: u32, bytes: &[u8]) -> u32 {
    let mut digest =
        crc_turbo::Digest::new_with_init_state(crc_turbo::CrcAlgorithm::Crc32Bzip2, u64::from(crc));
    digest.update(bytes);
    digest.get_state() as u32
}

#[cfg(test)]
fn crc32(bytes: &[u8]) -> u32 {
    !update_crc(!0, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bzip2_crc_check_value_and_incremental_updates() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xfc89_1918);
        assert_eq!(!update_crc(update_crc(!0, b"1234"), b"56789"), 0xfc89_1918);
    }

    #[test]
    fn crc_backend_matches_bitwise_polynomial_at_every_tail() {
        let data: Vec<_> = (0..521).map(|i| (i * 157 + i / 17) as u8).collect();
        for offset in 0..8 {
            for length in 0..=512 {
                let bytes = &data[offset..offset + length];
                let mut expected = !0u32;
                for &byte in bytes {
                    expected ^= u32::from(byte) << 24;
                    for _ in 0..8 {
                        expected = (expected << 1)
                            ^ if expected & 0x8000_0000 != 0 {
                                0x04c1_1db7
                            } else {
                                0
                            };
                    }
                }
                assert_eq!(update_crc(!0, bytes), expected);
            }
        }
    }

    #[test]
    fn crc_backend_preserves_carried_states_at_large_kernel_boundaries() {
        let data: Vec<_> = (0..65553).map(|i| (i * 157 + i / 17) as u8).collect();
        for offset in [0, 1, 7, 15] {
            for length in [1023, 1024, 1025, 4095, 4096, 4097, 65535, 65536, 65537] {
                let bytes = &data[offset..offset + length];
                for initial in [0, 0x1234_5678, !0u32] {
                    let mut expected = initial;
                    for &byte in bytes {
                        expected ^= u32::from(byte) << 24;
                        for _ in 0..8 {
                            expected = (expected << 1)
                                ^ if expected & 0x8000_0000 != 0 {
                                    0x04c1_1db7
                                } else {
                                    0
                                };
                        }
                    }
                    assert_eq!(update_crc(initial, bytes), expected);
                    let split = length / 3;
                    assert_eq!(
                        update_crc(update_crc(initial, &bytes[..split]), &bytes[split..]),
                        expected
                    );
                }
            }
        }
    }
}
