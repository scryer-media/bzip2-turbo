#![cfg(feature = "compress")]
use std::io::Write;

#[test]
fn public_streams_preserve_send_sync_and_unwind_traits() {
    fn assert_traits<
        T: Send + Sync + Unpin + std::panic::UnwindSafe + std::panic::RefUnwindSafe,
    >() {
    }
    assert_traits::<bzip2::Compress>();
    assert_traits::<bzip2::Decompress>();
    assert_traits::<bzip2_turbo::Compress>();
    assert_traits::<bzip2_turbo::Decompress>();
    assert_traits::<bzip2_turbo::read::BzEncoder<&[u8]>>();
    assert_traits::<bzip2_turbo::read::BzDecoder<&[u8]>>();
    assert_traits::<bzip2_turbo::write::BzEncoder<Vec<u8>>>();
    assert_traits::<bzip2_turbo::write::BzDecoder<Vec<u8>>>();
}

#[test]
fn bounded_mutations_match_upstream_raw_acceptance() {
    let input: Vec<_> = (0..4097).map(|i| (i * 137 + i / 31) as u8).collect();
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    encoder.write_all(&input).unwrap();
    let packed = encoder.finish().unwrap();
    let mut state = 0x7349_a12bu32;
    for case in 0..160 {
        let mut bytes = packed.clone();
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        if case < 128 {
            let at = state as usize % bytes.len();
            bytes[at] ^= 1 << ((state >> 16) % 8);
        } else if case < 159 {
            bytes.truncate(state as usize % bytes.len());
        }
        let mut native = bzip2_turbo::Decompress::new(case % 2 == 0);
        let mut oracle = bzip2::Decompress::new(case % 2 == 0);
        let mut native_output = vec![0; 8192];
        let mut oracle_output = vec![0; 8192];
        let a = native.decompress(&bytes, &mut native_output);
        let b = oracle.decompress(&bytes, &mut oracle_output);
        // Corrupt/truncated codes may fail before the oracle's lookahead can
        // decide. Successful stream acceptance must still agree exactly.
        assert_eq!(
            a == Ok(bzip2_turbo::Status::StreamEnd),
            b == Ok(bzip2::Status::StreamEnd),
            "case {case}"
        );
        if let (Err(a), Err(b)) = (a, b) {
            assert_eq!(format!("{a:?}"), format!("{b:?}"), "case {case}");
        }
        if a == Ok(bzip2_turbo::Status::StreamEnd) {
            assert_eq!(native.total_out(), oracle.total_out(), "case {case}");
            assert_eq!(
                &native_output[..native.total_out() as usize],
                &oracle_output[..oracle.total_out() as usize],
                "case {case}"
            );
        }
    }
}

#[test]
fn invalid_initial_huffman_length_cannot_be_repaired_by_deltas() {
    let mut bytes = Vec::new();
    let mut position = 0usize;
    let mut put = |value: u32, count: usize| {
        for shift in (0..count).rev() {
            if position.is_multiple_of(8) {
                bytes.push(0);
            }
            *bytes.last_mut().unwrap() |= (((value >> shift) & 1) as u8) << (7 - position % 8);
            position += 1;
        }
    };
    for byte in b"BZh1" {
        put(u32::from(*byte), 8);
    }
    put(0x314159, 24);
    put(0x265359, 24);
    put(0, 32);
    put(0, 1);
    put(0, 24);
    put(1 << 11, 16);
    put(1 << 14, 16); // Alphabet A.
    put(2, 3);
    put(1, 15);
    put(0, 1);
    put(0, 5); // Invalid initial length, followed by two increments to two.
    put(0b10100, 5);
    put(0, 2);
    put(20, 5);
    put(0, 3);
    put(0, 2);
    put(2, 2);
    let mut oracle = bzip2::Decompress::new(false);
    assert_eq!(
        oracle.decompress(&bytes, &mut [0; 16]),
        Err(bzip2::Error::Data)
    );
    let mut native = bzip2_turbo::Decompress::new(false);
    assert_eq!(
        native.decompress(&bytes, &mut [0; 16]),
        Err(bzip2_turbo::Error::Data)
    );
    assert_eq!(native.total_in(), oracle.total_in());
    assert_eq!(native.total_out(), oracle.total_out());
}
