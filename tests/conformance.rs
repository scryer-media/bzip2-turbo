#![cfg(feature = "compress")]

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use bzip2_turbo::{Decoder, Encoder, EncoderOptions};

fn oracle(input: &[u8], level: u32, threads: usize, encode: bool) -> Vec<u8> {
    oracle_with_options(input, level, threads, encode, &[])
}

fn oracle_with_options(
    input: &[u8],
    level: u32,
    threads: usize,
    encode: bool,
    options: &[String],
) -> Vec<u8> {
    let executable = std::env::var_os("BZIP2_TURBO_7ZZ").unwrap_or_else(|| "7zz".into());
    let mut command = Command::new(executable);
    if encode {
        command.args([
            "a",
            "-tbzip2",
            &format!("-mx={level}"),
            &format!("-mmt={threads}"),
            "-si",
            "-so",
            "stream.bz2",
        ]);
    } else {
        command.args(["x", "-tbzip2", "-si", "-so"]);
    }
    command.args(options);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("required 7zz oracle unavailable: install 7zz or set BZIP2_TURBO_7ZZ");
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        output.status.success(),
        "7zz failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn corpus() -> Vec<Vec<u8>> {
    let mut runs = Vec::new();
    for length in [1, 2, 3, 4, 5, 254, 255, 256, 257, 258, 259, 510, 65536] {
        runs.extend(std::iter::repeat_n(42, length));
        runs.push(17);
    }
    let mut state = 0x12345678u32;
    let random = (0..210_017)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    vec![
        vec![],
        vec![0],
        b"banana banana\n".repeat(31),
        (0..=255).cycle().take(4096).collect(),
        runs,
        random,
    ]
}

#[test]
fn seven_zip_encoding_is_byte_exact_at_every_level() {
    for (case, input) in corpus().iter().enumerate() {
        for level in 1..=9 {
            let expected = oracle(input, level, 1, true);
            for threads in [1, 2, 4] {
                let mut encoder = Encoder::new(
                    Vec::new(),
                    EncoderOptions {
                        level,
                        threads,
                        ..Default::default()
                    },
                );
                for chunk in input.chunks(137) {
                    encoder.write_all(chunk).unwrap();
                }
                let actual = encoder.finish().unwrap();
                assert_eq!(
                    actual, expected,
                    "case={case}, level={level}, threads={threads}"
                );
                assert_eq!(oracle(&actual, level, threads, false), *input);
            }
        }
    }
}

#[test]
fn seven_zip_streams_decode_with_short_reads_and_parallel_workers() {
    for input in corpus() {
        for level in [1, 5, 9] {
            let compressed = oracle(&input, level, 4, true);
            for threads in [1, 2, 4, 8] {
                let mut decoder = Decoder::with_threads(compressed.as_slice(), threads);
                let mut actual = Vec::new();
                let mut buffer = [0; 37];
                loop {
                    let count = decoder.read(&mut buffer).unwrap();
                    if count == 0 {
                        break;
                    }
                    actual.extend_from_slice(&buffer[..count]);
                }
                assert_eq!(actual, input);
            }
        }
    }
}

fn bits(data: &[u8], position: usize, count: usize) -> u32 {
    (position..position + count).fold(0, |value, bit| {
        (value << 1) | u32::from((data[bit / 8] >> (7 - bit % 8)) & 1)
    })
}

fn replace_bits(data: &[u8], position: usize, count: usize, value: u32) -> Vec<u8> {
    let mut changed = data.to_vec();
    for offset in 0..count {
        let bit = position + offset;
        let mask = 1 << (7 - bit % 8);
        changed[bit / 8] = (changed[bit / 8] & !mask)
            | (((value >> (count - offset - 1)) as u8 & 1) << (7 - bit % 8));
    }
    changed
}

#[test]
fn malformed_headers_and_truncated_streams_return_errors() {
    let packed = oracle(
        b"the complete block and stream checksums must be validated",
        1,
        1,
        true,
    );
    let map = 32 + 48 + 32 + 1 + 24;
    let group_flags = bits(&packed, map, 16);
    let table_position = map + 16 + group_flags.count_ones() as usize * 16;
    let mut malformed = Vec::new();
    for end in 0..packed.len() {
        malformed.push(packed[..end].to_vec());
    }
    malformed.push(replace_bits(&packed, 80, 32, 0)); // Block CRC.
    malformed.push(replace_bits(&packed, 113, 24, 0xffffff)); // BWT origin.
    for count in [0, 1, 7] {
        malformed.push(replace_bits(&packed, table_position, 3, count));
    }
    for count in [0, 32767] {
        malformed.push(replace_bits(&packed, table_position + 3, 15, count));
    }
    for (case, packed) in malformed.iter().enumerate() {
        for threads in [1, 4] {
            let result =
                Decoder::with_threads(packed.as_slice(), threads).read_to_end(&mut Vec::new());
            assert!(
                result.is_err(),
                "accepted malformed case {case} with {threads} workers"
            );
        }
    }
}

#[test]
fn legacy_randomized_blocks_match_the_reference() {
    // The historical randomizer's first toggle is byte 617; its next toggle
    // is beyond this block. Distinct adjacent bytes keep first-stage RLE inert.
    let original: Vec<u8> = (0..251).cycle().take(1000).map(|byte| byte as u8).collect();
    let mut randomized = original.clone();
    randomized[617] ^= 1;
    let packed = oracle(&randomized, 1, 1, true);
    let mut crc = !0u32;
    for &byte in &original {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x80000000 != 0 { 0x04c11db7 } else { 0 };
        }
    }
    let crc = !crc;
    let mut packed = replace_bits(&packed, 112, 1, 1);
    packed = replace_bits(&packed, 80, 32, crc);
    let trailer = (137..=packed.len() * 8 - 80)
        .find(|&position| {
            bits(&packed, position, 24) == 0x177245 && bits(&packed, position + 24, 24) == 0x385090
        })
        .expect("reference emitted one block and a trailer");
    packed = replace_bits(&packed, trailer + 48, 32, crc);
    assert_eq!(oracle(&packed, 1, 1, false), original);
    for threads in [1, 4] {
        let mut actual = Vec::new();
        Decoder::with_threads(packed.as_slice(), threads)
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, original);
    }
}

#[test]
fn explicit_block_sizes_and_optimization_passes_are_byte_exact() {
    let input: Vec<u8> = (0..310_017)
        .map(|index| {
            if index < 110_000 {
                (index * 173 + index / 17) as u8
            } else if index < 210_000 {
                b"subdivision candidates\n"[index % 23]
            } else {
                (index / 13) as u8
            }
        })
        .collect();
    for block_size in [100_000, 200_000, 900_000] {
        for passes in [1, 2, 3, 7, 10] {
            let expected = oracle_with_options(
                &input,
                5,
                1,
                true,
                &[format!("-md={block_size}b"), format!("-mpass={passes}")],
            );
            for threads in [1, 4] {
                let mut encoder = Encoder::new(
                    Vec::new(),
                    EncoderOptions {
                        level: 5,
                        threads,
                        block_size: Some(block_size),
                        passes: Some(passes),
                    },
                );
                encoder.write_all(&input).unwrap();
                let actual = encoder.finish().unwrap();
                assert!(
                    actual == expected,
                    "block_size={block_size}, passes={passes}, threads={threads}"
                );
            }
        }
    }
}

fn crc(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x80000000 != 0 { 0x04c11db7 } else { 0 };
        }
    }
    !crc
}

fn partial_huffman_stream(length: u32) -> Vec<u8> {
    let mut stream = Vec::new();
    let mut position = 0;
    let mut put = |value: u32, count: usize| {
        for offset in (0..count).rev() {
            if position % 8 == 0 {
                stream.push(0);
            }
            *stream.last_mut().unwrap() |= (((value >> offset) & 1) as u8) << (7 - position % 8);
            position += 1;
        }
    };
    for byte in b"BZh1" {
        put(u32::from(*byte), 8);
    }
    put(0x314159, 24);
    put(0x265359, 24);
    put(crc(b"A"), 32);
    put(0, 1);
    put(0, 24); // Not randomized, BWT origin zero.
    put(1 << 11, 16);
    put(1 << 14, 16); // Alphabet contains only 'A'.
    put(2, 3);
    put(1, 15);
    put(0, 1); // Two tables, one selector, table zero.
    put(length, 5);
    put(0, 3); // Three equally long codes.
    put(20, 5);
    put(0, 3); // An unused partial tree (lbzip2 compatibility).
    put(0, 2);
    put(2, 2); // RUNA (one 'A') then EOB with length two.
    put(0x177245, 24);
    put(0x385090, 24);
    put(crc(b"A"), 32);
    stream
}

#[test]
fn incomplete_huffman_maps_are_valid_but_oversubscribed_maps_are_not() {
    let valid = partial_huffman_stream(2);
    assert_eq!(oracle(&valid, 1, 1, false), b"A");
    for threads in [1, 4] {
        let mut output = Vec::new();
        Decoder::with_threads(valid.as_slice(), threads)
            .read_to_end(&mut output)
            .unwrap();
        assert_eq!(output, b"A");
        for length in [0, 1, 21, 31] {
            assert!(
                Decoder::with_threads(partial_huffman_stream(length).as_slice(), threads)
                    .read_to_end(&mut Vec::new())
                    .is_err(),
                "accepted lengths {length}"
            );
        }
    }
}

#[test]
fn concatenation_and_partial_next_headers_preserve_completed_stream_output() {
    let first = oracle(b"first", 1, 1, true);
    let second = oracle(b"second", 9, 1, true);
    for threads in [1, 4] {
        let mut packed = first.clone();
        packed.extend_from_slice(&second);
        let mut output = Vec::new();
        Decoder::with_threads(packed.as_slice(), threads)
            .read_to_end(&mut output)
            .unwrap();
        assert_eq!(output, b"firstsecond");
        for tail in [b"B".as_slice(), b"BZ", b"BZh", b"junk"] {
            let mut packed = first.clone();
            packed.extend_from_slice(tail);
            let result =
                Decoder::with_threads(packed.as_slice(), threads).read_to_end(&mut Vec::new());
            assert!(result.is_err(), "accepted trailing {tail:?}");
        }
    }
}

#[test]
fn encoder_matches_reference_on_periodic_and_run_block_boundaries() {
    for input in [
        vec![42; 3_000_017],
        (0..1_000_017).map(|i| (i % 2) as u8).collect(),
        (0..1_000_017).map(|i| (i % 251) as u8).collect(),
    ] {
        for level in [1, 9] {
            let expected = oracle(&input, level, 4, true);
            for threads in [1, 4] {
                let mut encoder = Encoder::new(
                    Vec::new(),
                    EncoderOptions {
                        level,
                        threads,
                        ..Default::default()
                    },
                );
                for part in input.chunks(65537) {
                    encoder.write_all(part).unwrap();
                    encoder.flush().unwrap();
                }
                assert!(
                    encoder.finish().unwrap() == expected,
                    "level={level}, threads={threads}"
                );
            }
        }
    }
}
