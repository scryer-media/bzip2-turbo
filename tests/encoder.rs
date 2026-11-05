#![cfg(feature = "compress")]

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use bzip2_turbo::{Encoder, EncoderOptions};

fn encoded(input: &[u8], threads: usize) -> Vec<u8> {
    let mut encoder = Encoder::new(
        Vec::new(),
        EncoderOptions {
            threads,
            passes: Some(1),
            ..Default::default()
        },
    );
    for part in input.chunks(137) {
        encoder.write_all(part).unwrap();
        encoder.flush().unwrap();
    }
    encoder.finish().unwrap()
}

#[test]
fn empty_stream_has_the_canonical_bytes() {
    assert_eq!(encoded(&[], 1), b"BZh9\x17\x72\x45\x38\x50\x90\0\0\0\0");
}

#[test]
fn default_options_match_reference_default_and_differ_from_level_nine() {
    let mut input = b"one stream has regions with different symbol statistics.\n".repeat(2048);
    let mut state = 0x12345678u32;
    for _ in 0..65536 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        input.push(state as u8);
    }
    let encode = |options| {
        let mut encoder = Encoder::new(Vec::new(), options);
        encoder.write_all(&input).unwrap();
        encoder.finish().unwrap()
    };
    let actual = encode(EncoderOptions::default());
    let high = encode(EncoderOptions {
        level: 9,
        ..Default::default()
    });
    assert!(
        actual != high,
        "fixture must distinguish one-pass and seven-pass defaults"
    );
    let executable = std::env::var_os("BZIP2_TURBO_7ZZ").unwrap_or_else(|| "7zz".into());
    let mut child = Command::new(executable)
        .args(["a", "-tbzip2", "-mmt=1", "-si", "-so", "stream.bz2"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("required 7zz default-encoder oracle unavailable");
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let reference = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        reference.status.success(),
        "7zz failed: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert!(
        actual == reference.stdout,
        "default encoder differs from reference default at byte {:?}",
        actual
            .iter()
            .zip(&reference.stdout)
            .position(|(a, b)| a != b)
    );
}

#[test]
fn runs_and_multiblock_round_trip_with_stable_thread_order() {
    let mut input = Vec::new();
    for length in [1, 2, 3, 4, 5, 254, 255, 256, 257, 258, 259, 510] {
        input.extend(std::iter::repeat_n(42, length));
        input.push(17);
    }
    let mut state = 1u32;
    for _ in 0..1_000_000 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        input.push(state as u8);
    }
    let single = encoded(&input, 1);
    assert_eq!(single, encoded(&input, 2));
    let mut decoded = Vec::new();
    bzip2_turbo::Decoder::new(single.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn flush_and_empty_writes_preserve_the_unfinished_block() {
    let mut encoder = Encoder::new(Vec::new(), EncoderOptions::default());
    assert_eq!(encoder.write(&[]).unwrap(), 0);
    encoder.write_all(b"streamed ").unwrap();
    encoder.flush().unwrap();
    assert_eq!(encoder.get_ref(), b"BZh9");
    encoder.write_all(b"bytes").unwrap();
    let fragmented = encoder.finish().unwrap();
    let mut whole = Encoder::new(Vec::new(), EncoderOptions::default());
    whole.write_all(b"streamed bytes").unwrap();
    assert_eq!(fragmented, whole.finish().unwrap());
}

#[test]
fn short_and_interrupted_sink_writes_are_retried() {
    struct ShortSink {
        bytes: Vec<u8>,
        interrupt: bool,
    }
    impl Write for ShortSink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.interrupt = !self.interrupt;
            if self.interrupt {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let count = bytes.len().min(3);
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let input = b"bounded partial writes and interrupted writes";
    let mut encoder = Encoder::new(
        ShortSink {
            bytes: Vec::new(),
            interrupt: false,
        },
        EncoderOptions {
            passes: Some(1),
            ..Default::default()
        },
    );
    encoder.write_all(input).unwrap();
    assert_eq!(encoder.finish().unwrap().bytes, encoded(input, 1));
}

#[test]
fn output_failures_poison_further_writes_and_finish() {
    struct Failed;
    impl Write for Failed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut encoder = Encoder::new(Failed, EncoderOptions::default());
    encoder.write_all(b"pending input").unwrap();
    assert_eq!(
        encoder.flush().unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert!(encoder.write(b"more").is_err());
    assert!(encoder.finish().is_err());
    let mut encoder = Encoder::new(
        Failed,
        EncoderOptions {
            level: 1,
            threads: 2,
            ..Default::default()
        },
    );
    let input: Vec<_> = (0..250_000).map(|i| i as u8).collect();
    let error = match encoder.write_all(&input) {
        Ok(()) => encoder.finish().err().unwrap(),
        Err(error) => {
            assert!(encoder.finish().is_err());
            error
        }
    };
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
}
