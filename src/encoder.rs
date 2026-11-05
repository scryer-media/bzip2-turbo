//! Block-bounded BZip2 writing.
// Translation of BZip2Encoder.cpp from 7-Zip, LGPL-2.1-or-later.
// https://github.com/ip7z/7zip/blob/9128b80e3a471108678e2b3ec3c985861c8d7b0f/CPP/7zip/Compress/BZip2Encoder.cpp
// Changes: safe Rust ownership, incremental Write interface, bounded ordered workers.

#[cfg(test)]
mod baseline;
mod entropy;
mod flag_sort;
mod marked_sort;
mod slot;
#[cfg(test)]
mod sort;
mod trial;

#[cfg(test)]
use baseline::{Bits, codes, encode_symbols, lengths, symbols, tables};
pub use slot::Encoder;

/// Compression settings. Worker results are always written in input order.
#[derive(Clone, Debug)]
pub struct EncoderOptions {
    /// Compression level, from zero through nine.
    pub level: u32,
    /// Thread budget including the calling thread, clamped to 1..=64.
    /// WebAssembly targets always use one thread.
    pub threads: usize,
    /// Requested block size in bytes, rounded down to a 100,000-byte step.
    /// `None` selects the level's block size; values are clamped to 1..=9 steps.
    pub block_size: Option<u32>,
    /// Recursive block optimization passes, clamped to 1..=10.
    /// `None` selects the level's pass count.
    pub passes: Option<u32>,
}

impl Default for EncoderOptions {
    fn default() -> Self {
        Self {
            level: 5,
            threads: 1,
            block_size: None,
            passes: None,
        }
    }
}

fn repeated_prefix(input: &[u8], byte: u8) -> usize {
    let repeated = u64::from_ne_bytes([byte; 8]);
    let mut position = 0;
    while position + 8 <= input.len() {
        if u64::from_ne_bytes(input[position..position + 8].try_into().unwrap()) != repeated {
            break;
        }
        position += 8;
    }
    while position < input.len() && input[position] == byte {
        position += 1;
    }
    position
}

fn find_and_move(alphabet: &mut [u64; 32], byte: u8) -> usize {
    let repeated = u64::from(byte) * 0x0101010101010101;
    let mut carry = u64::from(byte);
    for (index, entry) in alphabet.iter_mut().enumerate() {
        let previous = *entry;
        let different = previous ^ repeated;
        let matches = different.wrapping_sub(0x0101010101010101) & !different & 0x8080808080808080;
        if matches != 0 {
            let offset = matches.trailing_zeros() / 8;
            let mask = u64::MAX >> (56 - offset * 8);
            *entry = (previous & !mask) | (((previous << 8) | carry) & mask);
            return index * 8 + offset as usize;
        }
        *entry = (previous << 8) | carry;
        carry = previous >> 56;
    }
    unreachable!("BZip2 MTF input belongs to its byte alphabet")
}

fn block_crc(input: &[u8]) -> u32 {
    let mut crc = crc_turbo::Digest::new(crc_turbo::CrcAlgorithm::Crc32Bzip2);
    let mut position = 0;
    let mut literals = 0;
    while position < input.len() {
        let start = position;
        let byte = input[position];
        let mut count = 1;
        position += 1;
        while count < 4 && position < input.len() && input[position] == byte {
            count += 1;
            position += 1;
        }
        if count == 4 {
            if literals != start {
                crc.update(&input[literals..start]);
            }
            count += usize::from(input[position]);
            position += 1;
            crc.update(&[byte; 259][..count]);
            literals = position;
        }
    }
    if literals != input.len() {
        crc.update(&input[literals..]);
    }
    crc.finalize() as u32
}
