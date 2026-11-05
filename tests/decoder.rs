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
    assert_eq!(Decoder::<&[u8]>::low_memory_usage(1), 6 << 20);
    let parallel = if cfg!(target_family = "wasm") { 6 } else { 12 };
    assert_eq!(Decoder::<&[u8]>::low_memory_usage(8), parallel << 20);
    let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        10
    } else {
        6
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
fn many_small_blocks_share_one_worker_and_teardown_on_error_or_drop() {
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
    assert!(observed.1.iter().all(|id| *id == observed.1[0]));
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
