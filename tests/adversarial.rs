//! Inputs from published bzip2 advisories and the encoder's sorting worst
//! cases: each must round-trip or fail with an error, never hang or panic.
#![cfg(feature = "compress")]

use std::io::{Read, Write};

use bzip2_turbo::{Decoder, Decompress, Encoder, EncoderOptions, Status, read::BzDecoder};

fn xorshift(state: &mut u32) -> u8 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state as u8
}

fn noise(length: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..length).map(|_| xorshift(&mut state)).collect()
}

fn repeat(period: &[u8], length: usize) -> Vec<u8> {
    period.iter().copied().cycle().take(length).collect()
}

/// Inputs that defeat naive block sorters (CVE-2012-2098 class): short and
/// long periods, a 900,000-byte block that is an exact square, Fibonacci and
/// Thue-Morse words, and runs that sit on the four-byte RLE1 edge.
fn shapes() -> Vec<(&'static str, Vec<u8>)> {
    // Just over one 900,000-byte block, so every shape crosses a boundary.
    const LENGTH: usize = 1_100_000;
    let mut fibonacci = (b"a".to_vec(), b"ab".to_vec());
    while fibonacci.1.len() < LENGTH {
        let next = [fibonacci.1.as_slice(), &fibonacci.0].concat();
        fibonacci = (std::mem::take(&mut fibonacci.1), next);
    }
    fibonacci.1.truncate(LENGTH);
    let thue_morse = (0..LENGTH as u32)
        .map(|index| b'a' + (index.count_ones() % 2) as u8)
        .collect();
    vec![
        ("one byte", vec![b'a'; LENGTH]),
        ("period 2", repeat(b"ab", LENGTH)),
        ("period 3", repeat(b"abc", LENGTH)),
        ("period 71", repeat(&noise(71, 0x9e37_79b9), LENGTH)),
        ("period 2047", repeat(&noise(2047, 0x1234_5678), LENGTH)),
        (
            "half-block period",
            repeat(&noise(450_000, 0x0bad_f00d), LENGTH),
        ),
        ("fibonacci", fibonacci.1),
        ("thue-morse", thue_morse),
        ("rle1 edge", repeat(b"aaaab", LENGTH)),
        ("noise", noise(LENGTH, 0xdead_beef)),
    ]
}

fn encode(input: &[u8], threads: usize, passes: u32) -> Vec<u8> {
    let mut encoder = Encoder::new(
        Vec::new(),
        EncoderOptions {
            threads,
            level: 9,
            passes: Some(passes),
            ..Default::default()
        },
    );
    encoder.write_all(input).unwrap();
    encoder.finish().unwrap()
}

fn decode(encoded: &[u8], threads: usize) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    Decoder::with_threads(encoded, threads).read_to_end(&mut output)?;
    Ok(output)
}

#[test]
fn sorter_worst_case_shapes_round_trip() {
    for (name, input) in shapes() {
        let serial = encode(&input, 1, 1);
        assert_eq!(
            serial,
            encode(&input, 4, 1),
            "{name}: threaded bytes differ"
        );
        for threads in [1, 4] {
            assert_eq!(
                decode(&serial, threads).unwrap(),
                input,
                "{name}, {threads} threads"
            );
        }
        let mut compat = Vec::new();
        BzDecoder::new(serial.as_slice())
            .read_to_end(&mut compat)
            .unwrap();
        assert_eq!(compat, input, "{name}: compatibility decoder");
    }
}

#[test]
fn sorter_worst_case_shapes_round_trip_with_trials() {
    // Two passes run the recursive block-split trials and table optimisation.
    for (name, input) in shapes() {
        let encoded = encode(&input, 2, 2);
        assert_eq!(decode(&encoded, 1).unwrap(), input, "{name}");
    }
}

fn crc(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = (crc << 1)
                ^ if crc & 0x8000_0000 != 0 {
                    0x04c1_1db7
                } else {
                    0
                };
        }
    }
    !crc
}

/// A `BZh1` stream of one block over the alphabet `A`: `runs` RUNB symbols
/// and then EOB. Codes are RUNA `0`, RUNB `10` and EOB `11`. `expected` is
/// the output the stored block and stream CRCs describe.
fn run_stream(runs: usize, runa_first: bool, expected: &[u8]) -> Vec<u8> {
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
    put(crc(expected), 32);
    put(0, 1);
    put(0, 24); // Not randomized, BWT origin zero.
    put(1 << 11, 16);
    put(1 << 14, 16); // Alphabet contains only 'A'.
    let symbols = runs + usize::from(runa_first) + 1;
    let selectors = symbols.div_ceil(50);
    put(2, 3);
    put(selectors as u32, 15);
    for _ in 0..selectors {
        put(0, 1); // Table zero.
    }
    for _ in 0..2 {
        put(1, 5);
        put(0, 1); // RUNA: length 1.
        put(0b10, 2);
        put(0, 1); // RUNB: length 2.
        put(0, 1); // EOB: length 2.
    }
    if runa_first {
        put(0, 1);
    }
    for _ in 0..runs {
        put(0b10, 2);
    }
    put(0b11, 2);
    put(0x177245, 24);
    put(0x385090, 24);
    put(crc(expected), 32);
    stream
}

fn raw_decode(input: &[u8]) -> Result<Vec<u8>, bzip2_turbo::Error> {
    let mut decoder = Decompress::new(false);
    let mut output = vec![0; 1 << 20];
    let status = decoder.decompress(input, &mut output)?;
    assert_eq!(status, Status::StreamEnd);
    output.truncate(decoder.total_out() as usize);
    Ok(output)
}

#[test]
fn run_stream_builder_produces_a_valid_stream() {
    // RUNA then RUNB is a run of 1 + 2 * 2 = 5 'A's in the block. Undoing
    // RLE1 reads the fifth as a count: 4 + 65 = 69 'A's.
    let expected = &[b'A'; 69][..];
    let stream = run_stream(1, true, expected);
    for threads in [1, 4] {
        assert_eq!(decode(&stream, threads).unwrap(), expected);
    }
    assert_eq!(raw_decode(&stream).unwrap(), expected);
}

#[test]
fn runs_beyond_the_declared_block_size_are_errors() {
    // CVE-2010-0405: RUNA/RUNB runs overflowed libbz2's counters. Seventeen
    // RUNB symbols already exceed BZh1's 100,000 bytes; 200 would overflow
    // any machine word if the run were not bounded as it grows.
    for runs in [17, 64, 200] {
        let stream = run_stream(runs, false, b"");
        for threads in [1, 4] {
            let error = decode(&stream, threads).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{runs} runs");
        }
        assert!(raw_decode(&stream).is_err(), "{runs} runs, raw");
        assert!(
            BzDecoder::new(stream.as_slice())
                .read_to_end(&mut Vec::new())
                .is_err(),
            "{runs} runs, compatibility"
        );
    }
}

#[test]
fn a_failed_decoder_keeps_failing() {
    // CVE-2026-9669 class: reuse after an error must not resume decoding.
    let stream = run_stream(17, false, b"");
    for threads in [1, 4] {
        let mut decoder = Decoder::with_threads(stream.as_slice(), threads);
        let mut buffer = [0; 64];
        assert!(decoder.read(&mut buffer).is_err());
        assert!(decoder.read(&mut buffer).is_err());
    }
    let mut raw = Decompress::new(false);
    let mut output = [0; 64];
    assert!(raw.decompress(&stream, &mut output).is_err());
    assert_eq!(
        raw.decompress(&stream, &mut output),
        Err(bzip2_turbo::Error::Sequence)
    );
}

#[test]
fn empty_and_repeated_empty_streams_terminate() {
    // CVE-2015-7697 class: empty input and empty members must end, not loop.
    let empty = encode(&[], 1, 1);
    for threads in [1, 4] {
        assert!(decode(&[], threads).is_err());
        assert_eq!(decode(&empty, threads).unwrap(), b"");
        assert_eq!(decode(&empty.repeat(1000), threads).unwrap(), b"");
    }
}
