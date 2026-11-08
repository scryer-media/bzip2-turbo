use super::super::{Decoder, Mode};
use super::{
    BLOCK_MAGIC, INPUT_CHUNK, Input, LOOKAHEAD, OUTPUT_CAP, Pool, STREAM_END_MAGIC, Snapshot,
    retained, window,
};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

fn reference(data: &[u8], level: u32) -> Vec<u8> {
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(level));
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn reference_decode(encoded: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::new();
    bzip2::read::MultiBzDecoder::new(encoded)
        .read_to_end(&mut decoded)
        .unwrap();
    decoded
}

fn noise(len: usize, mut seed: u32, alphabet: &[u8]) -> Vec<u8> {
    (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            alphabet[(seed >> 8) as usize % alphabet.len()]
        })
        .collect()
}

fn bit(bytes: &[u8], at: usize) -> u64 {
    u64::from((bytes[at / 8] >> (7 - at % 8)) & 1)
}

fn bits(bytes: &[u8], at: usize, count: usize) -> u64 {
    (at..at + count).fold(0, |value, at| (value << 1) | bit(bytes, at))
}

/// Every bit offset at which a block magic starts, by brute force.
fn magics(bytes: &[u8]) -> Vec<u64> {
    let total = bytes.len() * 8;
    (0..total.saturating_sub(47))
        .filter(|&at| bits(bytes, at, 48) == BLOCK_MAGIC)
        .map(|at| at as u64)
        .collect()
}

/// Checks the documented bounds on one observation of a running pool.
fn check(snapshot: &Snapshot) {
    assert!(snapshot.workers <= snapshot.max_workers);
    assert!(snapshot.slots <= snapshot.max_slots);
    assert!(snapshot.buffered < OUTPUT_CAP + (1 << 20));
    assert!(snapshot.retained <= retained(snapshot.max_slots));
}

/// Reads to EOF with an odd buffer width, checking the pool after every read.
fn decode_checked(mut decoder: Decoder<&[u8]>) -> (Vec<u8>, bool) {
    let mut decoded = Vec::new();
    let mut buffer = vec![0; 40_009];
    loop {
        let count = decoder.read(&mut buffer).unwrap();
        if let Some(pool) = &decoder.pool {
            let snapshot = pool.snapshot();
            check(&snapshot);
            assert_eq!(snapshot.max_workers, decoder.threads);
        }
        if count == 0 {
            return (decoded, decoder.serial);
        }
        decoded.extend_from_slice(&buffer[..count]);
    }
}

#[test]
fn scanner_finds_every_magic_at_every_bit_offset_across_chunk_boundaries() {
    let mut bytes = noise(4096, 0x1234_5678, &(0..=255u8).collect::<Vec<_>>());
    // Plant magics at all eight alignments, adjacent pairs, and at the very
    // end, where only the EOF scan can see them.
    let mut plant = |at: usize| {
        for index in 0..48 {
            let value = (BLOCK_MAGIC >> (47 - index)) & 1;
            let position = at + index;
            let mask = 1 << (7 - position % 8);
            if value == 1 {
                bytes[position / 8] |= mask;
            } else {
                bytes[position / 8] &= !mask;
            }
        }
    };
    for shift in 0..8 {
        plant(800 * 8 + shift * 97 * 8 + shift);
    }
    plant(3000 * 8 + 3);
    plant(3000 * 8 + 3 + 48);
    plant(4096 * 8 - 48);
    plant(4096 * 8 - 100);
    let expected = magics(&bytes);
    assert!(expected.len() >= 12, "planted magics survive: {expected:?}");
    for width in [1, 2, 5, 7, 8, 9, 64, 1000, 4096] {
        // All chunks at once, then one chunk per scan with EOF last.
        for incremental in [false, true] {
            let mut input = Input::new();
            for chunk in bytes.chunks(width) {
                input.end += chunk.len() as u64;
                input.chunks.push_back(Arc::new(chunk.to_vec()));
                if incremental {
                    input.scan();
                }
            }
            input.eof = true;
            input.scan();
            assert_eq!(
                Vec::from(input.candidates.clone()),
                expected,
                "chunk width {width}, incremental {incremental}"
            );
            assert_eq!(input.scanned, bytes.len() as u64);
            // A reader from any start yields exactly the remaining bytes.
            for start in [0, 1, 799, 4095] {
                let mut read = Vec::new();
                input
                    .reader(start, u64::MAX)
                    .read_to_end(&mut read)
                    .unwrap();
                assert_eq!(read, &bytes[start as usize..]);
            }
        }
    }
}

#[test]
fn caller_stops_reading_at_the_window_and_releases_passed_input() {
    for threads in [2, 5] {
        let mut pool = Pool::new(threads, Mode::Compact, None, true);
        pool.started = true;
        pool.shared.lock().chain.expected = Some(0);
        // No magic anywhere: nothing is submitted and no worker starts.
        let mut zeros = std::io::repeat(0).take(1 << 40);
        pool.pump(&mut zeros).unwrap();
        let reached = pool.input.end;
        assert!(!pool.input.eof);
        assert!(reached > window(pool.max_slots), "stopped by the window");
        check(&pool.snapshot());
        // Calling again without progress reads nothing more.
        pool.pump(&mut zeros).unwrap();
        assert_eq!(pool.input.end, reached);
        // Once the chain moves on, passed input is released and reading
        // resumes up to the next window.
        let expected = reached - 3 * LOOKAHEAD as u64;
        pool.shared.lock().chain.expected = Some(expected * 8);
        pool.pump(&mut zeros).unwrap();
        assert!(pool.input.end > expected + window(pool.max_slots));
        assert!(pool.input.base > expected - INPUT_CHUNK as u64);
        let snapshot = pool.snapshot();
        check(&snapshot);
        assert_eq!(snapshot.workers, 0);
        assert_eq!(snapshot.slots, 0);
    }
}

#[test]
fn false_positive_magics_inside_block_data_are_discarded() {
    // A block's symbol map lists the used 16-byte groups, then a 16-bit mask
    // per used group. Using bytes from groups 0, 1 and 2 whose masks spell
    // the block magic plants it 121 bits after every real block magic.
    let mut alphabet = Vec::new();
    for (group, mask) in [0x3141u16, 0x5926, 0x5359].into_iter().enumerate() {
        for low in 0..16 {
            if mask & (1 << (15 - low)) != 0 {
                alphabet.push((group * 16 + low) as u8);
            }
        }
    }
    // No byte repeats, so run-length coding adds no length bytes to the map.
    let mut plain = noise(450_000, 0x0bad_cafe, &alphabet);
    for index in 1..plain.len() {
        if plain[index] == plain[index - 1] {
            let next = alphabet
                .iter()
                .position(|&byte| byte == plain[index])
                .unwrap()
                + 1;
            plain[index] = alphabet[next % alphabet.len()];
        }
    }
    let stream = reference(&plain, 1);
    let found = magics(&stream);
    let real: Vec<u64> = found
        .iter()
        .copied()
        .filter(|&at| {
            !at.checked_sub(121)
                .is_some_and(|before| found.contains(&before))
        })
        .collect();
    assert!(real.len() >= 4, "several blocks: {real:?}");
    for &at in &real {
        assert!(
            found.contains(&(at + 121)),
            "false positive after block at {at}"
        );
    }
    let encoded = [stream.as_slice(), stream.as_slice()].concat();
    let doubled = [plain.as_slice(), plain.as_slice()].concat();
    for threads in [1, 2, 3, 4, 8, 16] {
        let (decoded, serial) = decode_checked(Decoder::with_threads(encoded.as_slice(), threads));
        assert!(decoded == doubled, "{threads} threads");
        assert!(!serial);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed);
        let mut decoder =
            Decoder::with_output_callback(encoded.as_slice(), threads, move |bytes| {
                sink.lock().unwrap().extend_from_slice(bytes);
            });
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        drop(decoder);
        assert!(decoded == doubled);
        assert!(*observed.lock().unwrap() == doubled, "{threads} threads");
    }
}

#[test]
fn many_workers_deliver_straddling_blocks_in_order() {
    let alphabet: Vec<u8> = (0..64).map(|byte| byte * 3 + 7).collect();
    let mut plain = Vec::new();
    let mut encoded = Vec::new();
    // Three streams of small blocks, each block distinct; compressed blocks
    // straddle the caller's 1 MiB input chunks.
    for (seed, length) in [(1u32, 2_100_000), (2, 1), (3, 1_700_000)] {
        let part = noise(length, seed.wrapping_mul(0x9e37_79b9), &alphabet);
        encoded.extend(reference(&part, 1));
        plain.extend(part);
    }
    let starts: Vec<u64> = magics(&encoded).iter().map(|&at| at / 8).collect();
    assert!(starts.len() > 30);
    assert!(
        starts
            .windows(2)
            .filter(|pair| pair[0] / INPUT_CHUNK as u64 != pair[1] / INPUT_CHUNK as u64)
            .count()
            >= 2
    );
    for threads in [2, 3, 7, 16, 64] {
        let (decoded, serial) = decode_checked(Decoder::with_threads(encoded.as_slice(), threads));
        assert!(decoded == plain, "{threads} threads");
        assert!(!serial);
        let (decoded, _) = decode_checked(Decoder::with_low_memory(encoded.as_slice(), threads));
        assert!(decoded == plain, "{threads} threads, low memory");
        // Callback chunks arrive in stream order, one call at a time.
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed);
        let decoder = Decoder::with_output_callback(encoded.as_slice(), threads, move |bytes| {
            sink.lock().unwrap().extend_from_slice(bytes);
        });
        let (decoded, _) = decode_checked(decoder);
        assert!(decoded == plain);
        assert!(*observed.lock().unwrap() == plain, "{threads} threads");
    }
}

#[test]
fn dropping_a_busy_decoder_cancels_and_joins_every_worker() {
    let alphabet: Vec<u8> = (0..32).collect();
    let plain = noise(3_000_000, 77, &alphabet);
    let encoded = reference(&plain, 1);
    for threads in [2, 8, 16] {
        for read in [1, 70_000] {
            // Workers hold clones of the callback; once the decoder is
            // dropped, only this test's handle on the token may remain.
            let token = Arc::new(());
            let held = Arc::clone(&token);
            let mut decoder =
                Decoder::with_output_callback(encoded.as_slice(), threads, move |_| {
                    let _ = &held;
                });
            let mut buffer = vec![0; read];
            decoder.read_exact(&mut buffer).unwrap();
            assert_eq!(buffer, plain[..read]);
            let workers = decoder.pool.as_ref().unwrap().snapshot().workers;
            assert!(workers >= 1 && workers <= threads);
            drop(decoder);
            assert_eq!(Arc::strong_count(&token), 1, "{threads} threads");

            // Without a callback workers race ahead; `into_inner` joins them.
            let mut decoder = Decoder::with_threads(encoded.as_slice(), threads);
            decoder.read_exact(&mut buffer).unwrap();
            let shared = Arc::clone(&decoder.pool.as_ref().unwrap().shared);
            let rest = decoder.into_inner();
            assert!(rest.len() < encoded.len());
            assert_eq!(Arc::strong_count(&shared), 1, "{threads} threads");
        }
    }
}

/// Inserts `pairs` neutral Huffman length deltas (+1 then -1) into the first
/// table of a single-block stream, lengthening its encoding without changing
/// what it decodes to. Block and stream CRCs stay valid.
fn pad_first_table(stream: &[u8], pairs: usize) -> Vec<u8> {
    assert_eq!(pairs % 2, 0, "keep the trailer's byte alignment");
    let mut at = 32 + 48 + 32 + 1 + 24;
    let groups = bits(stream, at, 16);
    at += 16 + 16 * groups.count_ones() as usize;
    let selectors = bits(stream, at + 3, 15);
    at += 18;
    for _ in 0..selectors {
        while bit(stream, at) == 1 {
            at += 1;
        }
        at += 1;
    }
    let length = bits(stream, at, 5);
    at += 5;
    let pattern: u64 = if length < 20 { 0b1011 } else { 0b1110 };
    let total = stream.len() * 8 + pairs * 4;
    let mut out = vec![0u8; total / 8];
    let mut put = |position: usize, value: u64| {
        if value != 0 {
            out[position / 8] |= 1 << (7 - position % 8);
        }
    };
    for position in 0..at {
        put(position, bit(stream, position));
    }
    for index in 0..pairs * 4 {
        put(at + index, (pattern >> (3 - index % 4)) & 1);
    }
    for position in at..stream.len() * 8 {
        put(position + pairs * 4, bit(stream, position));
    }
    out
}

#[test]
fn a_block_encoded_beyond_the_lookahead_falls_back_to_serial_decoding() {
    let alphabet: Vec<u8> = (0..40).collect();
    let first = noise(250_000, 5, &alphabet);
    let middle = noise(40_000, 6, &alphabet);
    let last = noise(330_000, 7, &alphabet);
    // About 3.7 MB of neutral deltas: longer than a job's lookahead and the
    // partial chunk past it.
    let padded = pad_first_table(&reference(&middle, 9), 7_400_000);
    assert!(padded.len() > LOOKAHEAD + 2 * INPUT_CHUNK);
    assert!(reference_decode(&pad_first_table(&reference(&middle, 9), 1000)) == middle);
    let encoded = [reference(&first, 1), padded, reference(&last, 1)].concat();
    let plain = [first, middle, last].concat();
    for threads in [1, 2, 4, 16] {
        let (decoded, serial) = decode_checked(Decoder::with_threads(encoded.as_slice(), threads));
        assert!(decoded == plain, "{threads} threads");
        assert_eq!(serial, threads > 1);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed);
        let decoder = Decoder::with_output_callback(encoded.as_slice(), threads, move |bytes| {
            sink.lock().unwrap().extend_from_slice(bytes);
        });
        let (decoded, _) = decode_checked(decoder);
        assert!(decoded == plain);
        assert!(*observed.lock().unwrap() == plain, "{threads} threads");
    }
}

#[test]
fn corrupt_streams_fail_at_every_thread_count() {
    let alphabet: Vec<u8> = (0..48).collect();
    let plain = noise(520_000, 11, &alphabet);
    let stream = reference(&plain, 1);
    let blocks = magics(&stream);
    assert!(blocks.len() >= 5);
    let flip = |at: u64| {
        let mut bytes = stream.clone();
        bytes[(at / 8) as usize] ^= 1 << (7 - at % 8);
        bytes
    };
    // Each case, and whether its output before the error is exact: a
    // corrupt body can emit wrong bytes before its block CRC is checked.
    let mut cases = Vec::new();
    for cut in [
        1,
        4,
        10,
        stream.len() / 2,
        stream.len() - 5,
        stream.len() - 1,
    ] {
        cases.push((stream[..cut].to_vec(), true));
    }
    for &start in &blocks[1..4] {
        cases.push((flip(start + 48 + 7), true)); // Stored block CRC.
        cases.push((flip(start + 48 + 32 + 1 + 24 + 16 + 3000), false)); // Body.
        cases.push((flip(start + 20), true)); // Block magic.
    }
    let total = stream.len() * 8;
    let end = (total - 87..=total - 80)
        .find(|&at| bits(&stream, at, 48) == STREAM_END_MAGIC)
        .unwrap() as u64;
    cases.push((flip(end + 48 + 5), true)); // Stream CRC.
    cases.push((flip(end + 20), true)); // Stream end magic.
    for (case, (bytes, exact)) in cases.iter().enumerate() {
        for threads in [1, 2, 16] {
            let mut decoded = Vec::new();
            let result = Decoder::with_threads(bytes.as_slice(), threads).read_to_end(&mut decoded);
            assert!(result.is_err(), "case {case} with {threads} threads");
            assert!(decoded.len() <= plain.len());
            if *exact {
                assert!(
                    decoded == plain[..decoded.len()],
                    "case {case} with {threads} threads"
                );
            }
        }
    }
}
