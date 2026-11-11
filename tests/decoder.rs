use bzip2_turbo::Decoder;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn compress(data: &[u8]) -> Vec<u8> {
    compress_level(data, 9)
}
fn compress_level(data: &[u8], level: u32) -> Vec<u8> {
    let executable = std::env::var_os("BZIP2_TURBO_7ZZ").unwrap_or_else(|| "7zz".into());
    let mut child = Command::new(executable)
        .args([
            "a",
            "-tbzip2",
            &format!("-mx={level}"),
            "-mmt=1",
            "-si",
            "-so",
            "stream.bz2",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("7zz oracle; set BZIP2_TURBO_7ZZ to override");
    let input = child.stdin.take().unwrap();
    let data = data.to_vec();
    let writer = std::thread::spawn(move || {
        let mut input = input;
        input.write_all(&data).unwrap();
    });
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "7zz failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

#[test]
fn reference_streams_decode_at_every_output_boundary() {
    for data in [
        Vec::new(),
        b"banana banana\n".to_vec(),
        vec![42; 500_000],
        (0..300_000).map(|i| (i * 173 + i / 17) as u8).collect(),
    ] {
        let encoded = compress(&data);
        for threads in [1, 2, 4] {
            let mut decoder = Decoder::with_threads(encoded.as_slice(), threads);
            let mut decoded = Vec::new();
            let mut buf = [0; 37];
            loop {
                let n = decoder.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                decoded.extend_from_slice(&buf[..n]);
            }
            assert_eq!(decoded, data);
            for _ in 0..4 {
                assert_eq!(decoder.read(&mut []).unwrap(), 0);
                assert_eq!(decoder.read(&mut buf).unwrap(), 0);
            }
        }
    }
}

#[test]
fn compact_budget_factories_keep_callbacks_bytes_and_repeated_eof() {
    use std::sync::{Arc, Mutex};
    // Linux x86-64/AArch64 count the 2 MiB-rounded TT and last-column pages.
    let serial = if cfg!(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64"),
        not(miri)
    )) {
        8
    } else {
        6
    };
    assert_eq!(Decoder::<&[u8]>::low_memory_usage(1), serial << 20);
    // Eight workers: per worker a 4 MiB TT, a last column (one 2 MiB page on
    // Linux), 1 MiB of entropy scratch and one 1 MiB output chunk; 16 open
    // blocks of at most 3 MiB output; a read window of 18 lookaheads of
    // 1.25 MiB and two input chunks, eight job windows of 1.25 + 2 MiB and the
    // caller's output chunk.
    let column = if serial == 8 { 2 } else { 1 };
    let parallel = if cfg!(target_family = "wasm") {
        serial << 20
    } else {
        ((8 * (6 + column) + 48 + 1) << 20) + (18 + 8) * 1_310_720 + (2 + 16) * (1 << 20)
    };
    assert_eq!(Decoder::<&[u8]>::low_memory_usage(8), parallel);
    // The bound grows linearly with the budget and stops at 64 workers.
    if !cfg!(target_family = "wasm") {
        let step = Decoder::<&[u8]>::low_memory_usage(3) - Decoder::<&[u8]>::low_memory_usage(2);
        assert_eq!(
            Decoder::<&[u8]>::low_memory_usage(10) - Decoder::<&[u8]>::low_memory_usage(2),
            8 * step
        );
        assert_eq!(
            Decoder::<&[u8]>::low_memory_usage(65),
            Decoder::<&[u8]>::low_memory_usage(64)
        );
        // Multithreaded workers use compact words on every platform.
        assert_eq!(
            Decoder::<&[u8]>::memory_usage(8),
            Decoder::<&[u8]>::low_memory_usage(8)
        );
    }
    let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        10
    } else {
        serial
    };
    assert_eq!(Decoder::<&[u8]>::memory_usage(1), expected << 20);
    let data: Vec<_> = (0..300_001).map(|index| (index % 251) as u8).collect();
    let encoded = compress(&data);
    for threads in [1, 2, 4] {
        let mut decoder = Decoder::with_low_memory(encoded.as_slice(), threads);
        let mut plain = Vec::new();
        decoder.read_to_end(&mut plain).unwrap();
        assert_eq!(plain, data);
        for _ in 0..3 {
            assert_eq!(decoder.read(&mut []).unwrap(), 0);
            assert_eq!(decoder.read(&mut [0; 1]).unwrap(), 0);
        }
        let observed = Arc::new(Mutex::new(Vec::new()));
        let callback = Arc::clone(&observed);
        let mut decoder =
            Decoder::with_low_memory_output_callback(encoded.as_slice(), threads, move |bytes| {
                callback.lock().unwrap().extend_from_slice(bytes);
            });
        plain.clear();
        decoder.read_to_end(&mut plain).unwrap();
        drop(decoder);
        assert_eq!(plain, data);
        assert_eq!(*observed.lock().unwrap(), data);
    }
}

#[test]
fn truncations_and_bad_checksums_are_rejected() {
    let encoded = compress(b"the decoder must validate every bit of this stream");
    for end in 0..encoded.len() {
        let mut decoded = Vec::new();
        assert!(
            Decoder::new(&encoded[..end])
                .read_to_end(&mut decoded)
                .is_err(),
            "accepted prefix {end}"
        );
    }
    let mut corrupt = encoded;
    corrupt[10] ^= 1;
    assert!(
        Decoder::new(corrupt.as_slice())
            .read_to_end(&mut Vec::new())
            .is_err()
    );
}

#[test]
fn concatenated_streams_are_decoded() {
    let mut encoded = compress(b"first");
    encoded.extend(compress(b"second"));
    let mut decoded = Vec::new();
    Decoder::new(encoded.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, b"firstsecond");
}

#[test]
fn multi_block_stream_and_early_drop_join_the_pipeline() {
    let mut seed = 0x1357_2468u32;
    let data: Vec<u8> = (0..2_000_000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect();
    let encoded = compress(&data);
    for threads in [1, 2, 32] {
        let mut decoder = Decoder::with_threads(encoded.as_slice(), threads);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, data);
    }
    let mut decoder = Decoder::with_threads(encoded.as_slice(), 2);
    decoder.read_exact(&mut [0; 1]).unwrap();
    drop(decoder);
}

#[test]
fn mutations_never_panic_or_escape_block_bounds() {
    let encoded = compress(b"abccdefghijklmnopqrstuvwxyzz0123456789");
    for index in 0..encoded.len() {
        for mask in [1, 32, 128] {
            let mut mutated = encoded.clone();
            mutated[index] ^= mask;
            let mut decoder = Decoder::new(mutated.as_slice());
            let _ = std::io::copy(&mut decoder, &mut std::io::sink());
        }
    }
}

#[test]
fn output_callbacks_observe_producer_threads_and_every_chunk() {
    use std::sync::{Arc, Mutex};
    let data = b"producer-side checksums span all chunks\n".repeat(10_000);
    let encoded = compress(&data);
    let caller = std::thread::current().id();
    for threads in [1, 2] {
        let observed = Arc::new(Mutex::new((Vec::new(), Vec::new())));
        let callback_state = Arc::clone(&observed);
        let mut decoder =
            Decoder::with_output_callback(encoded.as_slice(), threads, move |bytes| {
                let mut state = callback_state.lock().unwrap();
                state.0.extend_from_slice(bytes);
                state.1.push(std::thread::current().id());
            });
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        drop(decoder);
        let state = observed.lock().unwrap();
        assert_eq!(decoded, data);
        assert_eq!(state.0, data);
        assert!(state.1.len() > 1);
        for &thread in &state.1 {
            if threads == 1 {
                assert_eq!(thread, caller);
            } else {
                assert_ne!(thread, caller);
            }
        }
    }
}

#[test]
fn callback_worker_panics_are_errors_and_early_drop_joins_ownership() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let encoded = compress(&vec![42; 2_000_000]);
    let mut decoder = Decoder::with_output_callback(encoded.as_slice(), 2, |_| {
        panic!("injected callback failure")
    });
    assert!(decoder.read(&mut [0; 1]).is_err());
    assert!(decoder.read(&mut [0; 1]).is_err());
    drop(decoder);
    let mut decoder = Decoder::with_output_callback(encoded.as_slice(), 1, |_| {
        panic!("injected caller callback failure")
    });
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decoder.read(&mut [0; 1])))
            .is_err()
    );
    assert!(decoder.read(&mut [0; 1]).is_err());
    drop(decoder);
    struct Owned(Arc<AtomicBool>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let owned = Owned(Arc::clone(&dropped));
    let mut decoder = Decoder::with_output_callback(encoded.as_slice(), 2, move |_| {
        let _ = &owned;
    });
    decoder.read_exact(&mut [0; 1]).unwrap();
    drop(decoder);
    assert!(dropped.load(Ordering::SeqCst));
    let mut corrupt = compress(b"small final chunk with a bad checksum");
    corrupt[10] ^= 1;
    let callbacks = Arc::new(AtomicBool::new(false));
    let state = Arc::clone(&callbacks);
    let mut decoder = Decoder::with_output_callback(corrupt.as_slice(), 2, move |_| {
        state.store(true, Ordering::SeqCst);
    });
    assert!(decoder.read_to_end(&mut Vec::new()).is_err());
    drop(decoder);
    assert!(!callbacks.load(Ordering::SeqCst));
}

#[test]
fn single_stream_mode_ignores_padding_but_validates_the_stream_crc() {
    let original = b"a 7z coder owns exactly one BZip2 stream";
    let encoded = compress(original);
    let mut padded = encoded.clone();
    padded.extend_from_slice(b"\0\0not another stream");
    for threads in [1, 2] {
        let mut decoder = Decoder::with_threads(padded.as_slice(), threads);
        decoder.set_multistream(false);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, original);
        assert!(
            Decoder::with_threads(padded.as_slice(), threads)
                .read_to_end(&mut Vec::new())
                .is_err()
        );
        let mut corrupt = encoded.clone();
        *corrupt.last_mut().unwrap() ^= 128;
        corrupt.extend_from_slice(b"\0padding");
        let mut decoder = Decoder::with_threads(corrupt.as_slice(), threads);
        decoder.set_multistream(false);
        assert!(decoder.read_to_end(&mut Vec::new()).is_err());
    }
}

#[test]
fn many_small_blocks_are_observed_on_workers_and_teardown_on_error_or_drop() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    let mut seed = 0x7152_3964u32;
    let data: Vec<u8> = (0..1_250_000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect();
    let encoded = compress_level(&data, 1);
    let observed = Arc::new(Mutex::new((Vec::new(), Vec::new())));
    let state = Arc::clone(&observed);
    let mut decoder = Decoder::with_output_callback(encoded.as_slice(), 2, move |bytes| {
        let mut state = state.lock().unwrap();
        state.0.extend_from_slice(bytes);
        state.1.push(std::thread::current().id());
    });
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded).unwrap();
    drop(decoder);
    let observed = observed.lock().unwrap();
    assert_eq!(decoded, data);
    assert_eq!(observed.0, data);
    assert!(observed.1.len() > 12);
    // Workers run the callback, one call at a time, never the caller.
    let caller = std::thread::current().id();
    assert!(observed.1.iter().all(|id| *id != caller));
    drop(observed);
    struct Owned(Arc<AtomicBool>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    for corrupt in [false, true] {
        let mut bytes = encoded.clone();
        if corrupt {
            let mut window = 0u64;
            let mut found = 0;
            let mut target = None;
            for bit in 0..bytes.len() * 8 {
                window = ((window << 1) | u64::from((bytes[bit / 8] >> (7 - bit % 8)) & 1))
                    & 0xFFFF_FFFF_FFFF;
                if window == 0x314159265359 {
                    found += 1;
                    if found == 2 {
                        target = Some(bit + 1);
                        break;
                    }
                }
            }
            let bit = target.expect("reference emitted multiple 100k blocks");
            bytes[bit / 8] ^= 1 << (7 - bit % 8);
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let owned = Owned(Arc::clone(&dropped));
        let mut decoder = Decoder::with_output_callback(bytes.as_slice(), 2, move |_| {
            let _ = &owned;
        });
        decoder.read_exact(&mut [0; 1]).unwrap();
        if corrupt {
            assert!(std::io::copy(&mut decoder, &mut std::io::sink()).is_err());
            assert!(decoder.read(&mut [0; 1]).is_err());
        }
        drop(decoder);
        assert!(dropped.load(Ordering::SeqCst));
    }
}

#[test]
fn fragmented_and_interrupted_input_preserves_word_refill_boundaries() {
    struct Fragmented<'a> {
        input: &'a [u8],
        calls: usize,
    }
    impl Read for Fragmented<'_> {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.calls.is_multiple_of(5) {
                return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
            }
            let count = output.len().min(1 + self.calls % 7);
            self.input.read(&mut output[..count])
        }
    }
    let data: Vec<u8> = (0..8193).map(|i| (i * 173 + i / 17) as u8).collect();
    let mut encoded = compress(&data);
    encoded.extend(compress(&data));
    for threads in [1, 2] {
        let mut decoder = Decoder::with_threads(
            Fragmented {
                input: &encoded,
                calls: 0,
            },
            threads,
        );
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, [data.as_slice(), data.as_slice()].concat());
    }
}

fn random_bytes(len: usize, mut seed: u32) -> Vec<u8> {
    (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect()
}

// The worker starts a fresh 64 KiB chunk sequence for every block and ends
// each block with a shorter chunk, so callback chunk sizes reveal the plain
// length of each block.
fn block_lengths(encoded: &[u8]) -> Vec<usize> {
    use std::sync::{Arc, Mutex};
    let chunks = Arc::new(Mutex::new(Vec::new()));
    let state = Arc::clone(&chunks);
    let mut decoder = Decoder::with_output_callback(encoded, 2, move |bytes| {
        state.lock().unwrap().push(bytes.len());
    });
    decoder.read_to_end(&mut Vec::new()).unwrap();
    drop(decoder);
    let mut blocks = Vec::new();
    let mut length = 0;
    for &chunk in chunks.lock().unwrap().iter() {
        length += chunk;
        if chunk < 65536 {
            blocks.push(length);
            length = 0;
        }
    }
    assert_eq!(length, 0, "a block ended on a whole chunk");
    blocks
}

// Bit offset just past the `ordinal`-th (1-based) block magic.
fn after_block_magic(bytes: &[u8], ordinal: usize) -> usize {
    let mut window = 0u64;
    let mut found = 0;
    for bit in 0..bytes.len() * 8 {
        window =
            ((window << 1) | u64::from((bytes[bit / 8] >> (7 - bit % 8)) & 1)) & 0xFFFF_FFFF_FFFF;
        if window == 0x314159265359 {
            found += 1;
            if found == ordinal {
                return bit + 1;
            }
        }
    }
    panic!("stream has fewer than {ordinal} blocks");
}

fn flip_bit(bytes: &mut [u8], bit: usize) {
    bytes[bit / 8] ^= 1 << (7 - bit % 8);
}

fn read_until_error(decoder: &mut impl Read, buffer: usize) -> (Vec<u8>, std::io::Error) {
    let mut decoded = Vec::new();
    let mut buf = vec![0; buffer];
    loop {
        match decoder.read(&mut buf) {
            Ok(0) => panic!("corrupt stream decoded to EOF"),
            Ok(n) => decoded.extend_from_slice(&buf[..n]),
            Err(error) => return (decoded, error),
        }
    }
}

#[test]
fn parser_error_after_queued_blocks_delivers_their_output_first() {
    let data = random_bytes(600_000, 0x2468_1357);
    let encoded = compress_level(&data, 1);
    let blocks = block_lengths(&encoded);
    assert!(blocks.len() >= 5);
    let prefix = blocks[0] + blocks[1];
    let mut corrupt = encoded.clone();
    // Break the third block's marker. Whether serial or on workers, the first
    // two blocks decode and the error is found where the third should start;
    // their output must still be delivered before the error.
    let bit = after_block_magic(&corrupt, 3) - 1;
    flip_bit(&mut corrupt, bit);
    for threads in [1, 2, 4] {
        for buffer in [1, 37, 1 << 20] {
            let mut decoder = Decoder::with_threads(corrupt.as_slice(), threads);
            let (decoded, error) = read_until_error(&mut decoder, buffer);
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert_eq!(decoded.len(), prefix, "threads {threads} buffer {buffer}");
            assert_eq!(decoded, data[..prefix]);
            assert!(decoder.read(&mut [0; 1]).is_err());
        }
    }
}

#[test]
fn worker_crc_error_never_delivers_the_queued_successor() {
    use std::sync::{Arc, Mutex};
    let data = random_bytes(600_000, 0x0bad_c0de);
    let encoded = compress_level(&data, 1);
    let blocks = block_lengths(&encoded);
    assert!(blocks.len() >= 4);
    for victim in [0, 1] {
        let mut corrupt = encoded.clone();
        // The stored block CRC follows the marker; only reconstruction checks
        // it, after the parser has queued the next block.
        let bit = after_block_magic(&corrupt, victim + 1) + 7;
        flip_bit(&mut corrupt, bit);
        let limit: usize = blocks[..=victim].iter().sum();
        let start: usize = blocks[..victim].iter().sum();
        for threads in [1, 2, 4] {
            let observed = Arc::new(Mutex::new(0usize));
            let state = Arc::clone(&observed);
            let mut decoder =
                Decoder::with_output_callback(corrupt.as_slice(), threads, move |bytes| {
                    *state.lock().unwrap() += bytes.len();
                });
            // With a worker, the first read returns only after the successor
            // is queued (or the worker has already stopped on the bad CRC); the
            // pause lets the worker reach the bad CRC with it still waiting,
            // so a worker that carried on would deliver it before the error.
            let mut first = [0; 1];
            decoder.read_exact(&mut first).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
            let (rest, error) = read_until_error(&mut decoder, 4096);
            let decoded = [first.as_slice(), &rest].concat();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert!(error.to_string().contains("block CRC"), "{error}");
            drop(decoder);
            assert!(decoded.len() >= start && decoded.len() < limit);
            assert_eq!(decoded, data[..decoded.len()]);
            assert!(*observed.lock().unwrap() < limit);
        }
    }
}

#[test]
fn worker_reconstructs_the_queued_successor_while_the_caller_waits() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::{Duration, Instant};
    // Full 100k blocks, then one-block streams small enough that the caller
    // parses the successor before the worker has taken the first block.
    let full = compress_level(&random_bytes(350_000, 0x0a5c_ade5), 1);
    let mut tiny = Vec::new();
    for text in [b"first".as_slice(), b"second", b"third"] {
        tiny.extend(compress_level(text, 1));
    }
    for (encoded, rounds) in [(&full, 1), (&tiny, 20)] {
        let blocks = block_lengths(encoded);
        assert!(blocks.len() >= 3);
        let target = blocks[0] + blocks[1];
        for _ in 0..rounds {
            let observed = Arc::new(AtomicUsize::new(0));
            let state = Arc::clone(&observed);
            let mut decoder = Decoder::with_output_callback(encoded.as_slice(), 2, move |bytes| {
                state.fetch_add(bytes.len(), Ordering::SeqCst);
            });
            decoder.read_exact(&mut [0; 1]).unwrap();
            // The caller stays paused: only a successor queued during the
            // first read can be reconstructed now. The output ring holds both
            // blocks' chunks, so the worker never waits for this thread.
            let deadline = Instant::now() + Duration::from_secs(5);
            while observed.load(Ordering::SeqCst) < target && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            // Up to twice the budget of blocks may be decoded ahead.
            assert!(
                observed.load(Ordering::SeqCst) >= target,
                "the second block was not reconstructed ahead of the caller"
            );
            drop(decoder);
        }
    }
}

#[test]
fn tiny_final_blocks_follow_full_blocks() {
    let source = random_bytes(400_000, 0x5eed_1234);
    let first = block_lengths(&compress_level(&source, 1))[0];
    for tail in [1, 2, 3, 17] {
        let data = &source[..first * 2 + tail];
        let encoded = compress_level(data, 1);
        assert_eq!(block_lengths(&encoded), [first, first, tail]);
        let mut concatenated = encoded.clone();
        concatenated.extend(compress_level(&data[..tail], 1));
        let expected = [data, &data[..tail]].concat();
        for threads in [1, 2, 4] {
            for buffer in [1, 4096, 1 << 20] {
                for (stream, plain) in [(&encoded, data), (&concatenated, expected.as_slice())] {
                    let mut decoder = Decoder::with_threads(stream.as_slice(), threads);
                    let mut decoded = Vec::new();
                    let mut buf = vec![0; buffer];
                    loop {
                        let n = decoder.read(&mut buf).unwrap();
                        if n == 0 {
                            break;
                        }
                        decoded.extend_from_slice(&buf[..n]);
                    }
                    assert_eq!(decoded, plain, "tail {tail} threads {threads}");
                    assert_eq!(decoder.read(&mut buf).unwrap(), 0);
                }
            }
        }
    }
}

#[test]
fn one_byte_reads_match_bulk_reads_across_blocks() {
    let mut data = random_bytes(250_000, 0x1234_abcd);
    data.extend(std::iter::repeat_n(7u8, 300_000));
    data.extend(random_bytes(90_000, 0x4321_dcba));
    let encoded = compress_level(&data, 1);
    assert!(block_lengths(&encoded).len() >= 4);
    for threads in [1, 2, 4] {
        for low_memory in [false, true] {
            let mut decoder = if low_memory {
                Decoder::with_low_memory(encoded.as_slice(), threads)
            } else {
                Decoder::with_threads(encoded.as_slice(), threads)
            };
            let mut decoded = Vec::with_capacity(data.len());
            let mut byte = [0; 1];
            loop {
                match decoder.read(&mut byte).unwrap() {
                    0 => break,
                    n => decoded.extend_from_slice(&byte[..n]),
                }
            }
            assert!(decoded == data, "threads {threads} low memory {low_memory}");
            assert_eq!(decoder.read(&mut byte).unwrap(), 0);
        }
    }
}

/// CRC-32/ISO-HDLC, bit by bit.
fn crc32_oracle(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Decodes through a CRC-32 callback and returns what was delivered, what was
/// observed, and whether every chunk's checksum matched its bytes.
fn decode_crc32(encoded: &[u8], threads: usize, low_memory: bool) -> (Vec<u8>, Vec<u8>, usize) {
    use std::sync::{Arc, Mutex};
    let observed = Arc::new(Mutex::new((Vec::new(), 0usize)));
    let state = Arc::clone(&observed);
    let callback = move |bytes: &[u8], crc: u32| {
        assert!(bytes.len() <= 65536);
        assert_eq!(crc, crc32_oracle(bytes));
        let mut state = state.lock().unwrap();
        state.0.extend_from_slice(bytes);
        state.1 += 1;
        Ok(())
    };
    let mut decoder = if low_memory {
        Decoder::with_low_memory_crc32_output_callback(encoded, threads, callback)
    } else {
        Decoder::with_crc32_output_callback(encoded, threads, callback)
    };
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded).unwrap();
    drop(decoder);
    let (bytes, calls) = std::mem::take(&mut *observed.lock().unwrap());
    (decoded, bytes, calls)
}

#[test]
fn crc32_callbacks_see_every_chunk_in_order_with_its_checksum() {
    let blocks = random_bytes(1_250_000, 0x1357_9bdf);
    let mut streams = Vec::new();
    let mut plain = Vec::new();
    for part in [&blocks[..700_000], b"x".as_slice(), &[], &blocks[700_000..]] {
        streams.extend(compress_level(part, 1));
        plain.extend_from_slice(part);
    }
    let single = b"one block, one stream".to_vec();
    for (encoded, data) in [
        (compress_level(&blocks, 1), blocks.clone()),
        (streams, plain),
        (compress(&single), single),
        (compress(b"a"), b"a".to_vec()),
        (compress(&[]), Vec::new()),
    ] {
        for threads in [1, 2, 3, 7, 16, 64] {
            for low_memory in [false, true] {
                let (decoded, observed, calls) = decode_crc32(&encoded, threads, low_memory);
                assert!(decoded == data, "threads {threads}");
                assert!(observed == data, "threads {threads}");
                assert_eq!(calls == 0, data.is_empty());
            }
        }
    }
}

#[test]
fn a_damaged_middle_block_is_observed_up_to_its_start_then_fails() {
    use std::sync::{Arc, Mutex};
    let data = random_bytes(600_000, 0x2468_1357);
    let encoded = compress_level(&data, 1);
    let blocks = block_lengths(&encoded);
    assert!(blocks.len() >= 5);
    // A broken third block marker: nothing of it is decoded.
    let prefix = blocks[0] + blocks[1];
    let mut marker = encoded.clone();
    let bit = after_block_magic(&marker, 3) - 1;
    flip_bit(&mut marker, bit);
    // A broken third block CRC: its bytes are produced, then refused.
    let mut crc = encoded.clone();
    let bit = after_block_magic(&crc, 3) + 7;
    flip_bit(&mut crc, bit);
    for threads in [1, 2, 4, 16] {
        for (corrupt, exact) in [(&marker, true), (&crc, false)] {
            let observed = Arc::new(Mutex::new(Vec::new()));
            let state = Arc::clone(&observed);
            let mut decoder =
                Decoder::with_crc32_output_callback(corrupt.as_slice(), threads, move |b, c| {
                    assert_eq!(c, crc32_oracle(b));
                    state.lock().unwrap().extend_from_slice(b);
                    Ok(())
                });
            let (decoded, error) = read_until_error(&mut decoder, 4096);
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert!(decoder.read(&mut [0; 1]).is_err());
            drop(decoder);
            let observed = observed.lock().unwrap();
            // The callback saw exactly what was delivered, nothing after it.
            assert!(*observed == decoded, "threads {threads}");
            if exact {
                assert_eq!(decoded.len(), prefix, "threads {threads}");
            } else {
                assert!(error.to_string().contains("block CRC"), "{error}");
                assert!(decoded.len() >= prefix && decoded.len() < prefix + blocks[2]);
            }
            assert!(decoded == data[..decoded.len()]);
        }
    }
}

#[test]
fn a_crc32_callback_error_stops_the_decode_at_that_chunk() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let data = random_bytes(900_000, 0x0f1e_2d3c);
    let encoded = compress_level(&data, 1);
    for threads in [1, 2, 4, 16] {
        for accept in [0usize, 1, 3, 11] {
            let calls = Arc::new(AtomicUsize::new(0));
            let accepted = Arc::new(AtomicUsize::new(0));
            let (counter, total) = (Arc::clone(&calls), Arc::clone(&accepted));
            let mut decoder =
                Decoder::with_crc32_output_callback(encoded.as_slice(), threads, move |b, _| {
                    if counter.fetch_add(1, Ordering::SeqCst) == accept {
                        return Err(std::io::Error::other("refused by the consumer"));
                    }
                    total.fetch_add(b.len(), Ordering::SeqCst);
                    Ok(())
                });
            let (decoded, error) = read_until_error(&mut decoder, 7777);
            assert_eq!(error.to_string(), "refused by the consumer");
            assert!(decoder.read(&mut [0; 1]).is_err());
            drop(decoder);
            // The refused chunk and everything after it are neither delivered
            // nor observed.
            assert_eq!(
                calls.load(Ordering::SeqCst),
                accept + 1,
                "threads {threads}"
            );
            assert_eq!(decoded.len(), accepted.load(Ordering::SeqCst));
            assert!(decoded == data[..decoded.len()]);
            if accept > 0 {
                assert!(!decoded.is_empty());
            }
        }
    }
}
