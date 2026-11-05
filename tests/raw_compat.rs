use bzip2_turbo::{Decompress, Error, Status};
#[cfg(feature = "compress")]
use std::io::Read;
use std::{io::Write, mem::MaybeUninit};

fn input(size: usize) -> Vec<u8> {
    let mut state = 0x5a17_0d93u32;
    (0..size)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            if i % 997 < 100 {
                (i % 17) as u8
            } else {
                state as u8
            }
        })
        .collect()
}
fn reference(bytes: &[u8], level: u32) -> Vec<u8> {
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(level));
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
fn decode_chunks(encoded: &[u8], expected: &[u8], chunk: usize, small: bool) {
    let mut decoder = Decompress::new(small);
    let mut actual = Vec::new();
    let mut output = [0; 8191];
    let mut position = 0;
    loop {
        let end = encoded.len().min(position + chunk);
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(&encoded[position..end], &mut output)
            .unwrap();
        position += (decoder.total_in() - before_in) as usize;
        actual.extend_from_slice(&output[..(decoder.total_out() - before_out) as usize]);
        if status == Status::StreamEnd {
            break;
        }
        assert!(
            decoder.total_in() != before_in || decoder.total_out() != before_out,
            "stalled at {position}"
        );
    }
    assert_eq!(position, encoded.len());
    assert_eq!(actual, expected);
    assert_eq!(decoder.decompress(&[], &mut output), Err(Error::Sequence));
}
#[test]
fn incremental_parser_handles_single_byte_full_blocks_and_all_alphabets() {
    // A level-nine block traverses a full alphabet, every selector boundary,
    // reservoir split and a second block while keeping one-byte input calls linear.
    let bytes = input(910_003);
    let encoded = reference(&bytes, 9);
    decode_chunks(&encoded, &bytes, 1, false);
    decode_chunks(&encoded, &bytes, 32768, true);
}
#[test]
fn incremental_parser_preserves_every_short_boundary_and_member_end() {
    for bytes in [
        Vec::new(),
        b"abcabcabc".to_vec(),
        vec![255; 10001],
        input(4097),
    ] {
        let encoded = reference(&bytes, 1);
        for chunk in [1, 2, 3, 7, 31, 4096] {
            decode_chunks(&encoded, &bytes, chunk, false);
        }
        let joined = [encoded.as_slice(), b"trailing bytes"].concat();
        let mut decoder = Decompress::new(false);
        let mut actual = vec![0; bytes.len() + 1];
        assert_eq!(
            decoder.decompress(&joined, &mut actual).unwrap(),
            Status::StreamEnd
        );
        assert_eq!(decoder.total_in() as usize, encoded.len());
    }
}
#[test]
fn raw_errors_distinguish_magic_and_corruption() {
    assert_eq!(
        Decompress::new(false).decompress(b"BAD!", &mut [0; 16]),
        Err(Error::DataMagic)
    );
    let mut encoded = reference(b"corrupt checksum", 1);
    encoded[10] ^= 1;
    assert_eq!(
        Decompress::new(false).decompress(&encoded, &mut [0; 100]),
        Err(Error::Data)
    );
}
#[test]
fn uninitialized_and_spare_capacity_outputs_track_only_written_bytes() {
    let bytes = input(1024);
    let encoded = reference(&bytes, 1);
    let mut decoder = Decompress::new(false);
    let mut uninit = [MaybeUninit::uninit(); 1025];
    assert_eq!(
        decoder.decompress_uninit(&encoded, &mut uninit).unwrap(),
        Status::StreamEnd
    );
    assert_eq!(decoder.total_out(), 1024);
    let mut decoder = Decompress::new(false);
    let mut output = Vec::with_capacity(1025);
    output.push(42);
    decoder.decompress_vec(&encoded, &mut output).unwrap();
    assert_eq!(&output[1..], bytes);
}
#[cfg(feature = "compress")]
#[test]
fn compression_finishes_with_tiny_outputs_and_validates_sequence() {
    use bzip2_turbo::{Action, Compress, Compression};
    let bytes = input(210_003);
    let mut compressor = Compress::new(Compression::fast(), 30);
    let mut position = 0;
    let mut encoded = Vec::new();
    let mut output = [0; 7];
    loop {
        let before_in = compressor.total_in();
        let before_out = compressor.total_out();
        let status = compressor
            .compress(&bytes[position..], &mut output, Action::Finish)
            .unwrap();
        position += (compressor.total_in() - before_in) as usize;
        encoded.extend_from_slice(&output[..(compressor.total_out() - before_out) as usize]);
        if status == Status::StreamEnd {
            break;
        }
        assert_eq!(
            compressor.compress(&[], &mut output, Action::Flush),
            Err(Error::Sequence)
        );
    }
    assert_eq!(position, bytes.len());
    let mut actual = Vec::new();
    bzip2::read::BzDecoder::new(encoded.as_slice())
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, bytes);
}
#[cfg(feature = "compress")]
#[test]
fn compression_flush_continues_and_preserves_run_boundaries() {
    use bzip2_turbo::{Action, Compress, Compression};
    let mut compressor = Compress::new(Compression::best(), 0);
    let mut encoded = Vec::new();
    for (bytes, action) in [
        (vec![7; 10000], Action::Flush),
        (input(131_007), Action::Finish),
    ] {
        let mut position = 0;
        loop {
            let before_in = compressor.total_in();
            let before_out = compressor.total_out();
            let mut output = [0; 1024];
            let status = compressor
                .compress(&bytes[position..], &mut output, action)
                .unwrap();
            position += (compressor.total_in() - before_in) as usize;
            encoded.extend_from_slice(&output[..(compressor.total_out() - before_out) as usize]);
            if matches!(status, Status::RunOk | Status::StreamEnd) {
                break;
            }
        }
        assert_eq!(position, bytes.len());
    }
    let mut actual = Vec::new();
    bzip2::read::BzDecoder::new(encoded.as_slice())
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, [vec![7; 10000], input(131_007)].concat());
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
    let mut position = 0usize;
    let mut put = |value: u32, count: usize| {
        for offset in (0..count).rev() {
            if position.is_multiple_of(8) {
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
fn raw_legacy_randomizer_and_partial_huffman_trees_match_reference() {
    let original: Vec<u8> = (0..251).cycle().take(1000).map(|byte| byte as u8).collect();
    let mut randomized = original.clone();
    randomized[617] ^= 1;
    let packed = reference(&randomized, 1);
    let mut packed = replace_bits(&packed, 112, 1, 1);
    packed = replace_bits(&packed, 80, 32, crc(&original));
    let trailer = (137..=packed.len() * 8 - 80)
        .find(|&position| {
            bits(&packed, position, 24) == 0x177245 && bits(&packed, position + 24, 24) == 0x385090
        })
        .unwrap();
    packed = replace_bits(&packed, trailer + 48, 32, crc(&original));
    let mut oracle = bzip2::Decompress::new(false);
    let mut output = [0; 1001];
    assert_eq!(
        oracle.decompress(&packed, &mut output).unwrap(),
        bzip2::Status::StreamEnd
    );
    assert_eq!(&output[..1000], original);
    for small in [false, true] {
        decode_chunks(&packed, &original, 1, small);
    }
    let valid = partial_huffman_stream(2);
    decode_chunks(&valid, b"A", 1, false);
    decode_chunks(&valid, b"A", 2, true);
    for length in [0, 1, 21, 31] {
        let invalid = partial_huffman_stream(length);
        assert_eq!(
            Decompress::new(false).decompress(&invalid, &mut [0; 16]),
            Err(Error::Data)
        );
    }
}
#[test]
fn checksum_error_keeps_delivered_bytes_and_counters_like_reference() {
    let original = input(4097);
    let mut encoded = reference(&original, 1);
    encoded[10] ^= 1;
    let mut oracle = bzip2::Decompress::new(false);
    let mut expected = vec![0; original.len() + 1];
    assert_eq!(
        oracle.decompress(&encoded, &mut expected),
        Err(bzip2::Error::Data)
    );
    for small in [false, true] {
        let mut decoder = Decompress::new(small);
        let mut actual = vec![0; original.len() + 1];
        assert_eq!(decoder.decompress(&encoded, &mut actual), Err(Error::Data));
        assert_eq!(decoder.total_out(), oracle.total_out());
        assert_eq!(
            &actual[..decoder.total_out() as usize],
            &expected[..oracle.total_out() as usize]
        );
        let mut decoder = Decompress::new(small);
        let mut actual = Vec::with_capacity(original.len() + 1);
        assert_eq!(
            decoder.decompress_vec(&encoded, &mut actual),
            Err(Error::Data)
        );
        assert_eq!(actual, expected[..oracle.total_out() as usize]);
    }
}
#[test]
fn raw_parser_mutations_and_arbitrary_inputs_are_bounded() {
    let encoded = reference(b"abccdefghijklmnopqrstuvwxyzz0123456789", 1);
    let mut cases = Vec::new();
    for index in 0..encoded.len() {
        for mask in [1, 32, 128] {
            let mut mutated = encoded.clone();
            mutated[index] ^= mask;
            cases.push(mutated);
        }
    }
    for size in 0..128 {
        cases.push(input(size));
    }
    for encoded in cases {
        for small in [false, true] {
            let mut decoder = Decompress::new(small);
            let mut position = 0;
            let mut produced = 0;
            loop {
                let before_in = decoder.total_in();
                let before_out = decoder.total_out();
                let mut output = [0; 113];
                let end = encoded.len().min(position + 1);
                let result = decoder.decompress(&encoded[position..end], &mut output);
                position += (decoder.total_in() - before_in) as usize;
                produced += decoder.total_out() - before_out;
                assert!(position <= encoded.len());
                assert!(produced <= 50_000_000);
                if result.is_err() || result == Ok(Status::StreamEnd) {
                    break;
                }
                if decoder.total_in() == before_in && decoder.total_out() == before_out {
                    break;
                }
            }
        }
    }
}
