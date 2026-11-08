use super::{COLUMN_OWNERS, Decoder};

fn members(levels: &[u32], length: usize) -> (Vec<u8>, Vec<u8>) {
    use std::io::Write;
    let mut plain = Vec::new();
    let mut encoded = Vec::new();
    for (member, &level) in levels.iter().enumerate() {
        let data: Vec<u8> = (0..length)
            .map(|i| ((i * 131 + member * 7) ^ (i >> 9)) as u8)
            .collect();
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(level));
        encoder.write_all(&data).unwrap();
        encoded.extend(encoder.finish().unwrap());
        plain.extend(data);
    }
    (encoded, plain)
}

#[test]
fn last_column_owners_never_exceed_their_bound_and_are_reused() {
    use std::io::Read;
    // Many level-1 blocks, then a level-9 member whose limit regrows the
    // recycled owners, then level 1 again.
    let (encoded, plain) = members(&[1, 9, 1], 650_003);
    for threads in [1, 2, 4] {
        for low_memory in [false, true] {
            for width in [1, 4093, 1 << 20] {
                let mut decoder = if low_memory {
                    Decoder::with_low_memory(encoded.as_slice(), threads)
                } else {
                    Decoder::with_threads(encoded.as_slice(), threads)
                };
                let mut decoded = Vec::with_capacity(plain.len());
                let mut buffer = vec![0; width];
                loop {
                    let n = decoder.read(&mut buffer).unwrap();
                    // Workers own their columns; the serial parser's pool
                    // is used only with one thread.
                    let owners = decoder.parser.columns.owners;
                    assert!(owners <= if threads == 1 { COLUMN_OWNERS } else { 0 });
                    assert!(owners >= decoder.parser.columns.spare.len());
                    if n == 0 {
                        break;
                    }
                    decoded.extend_from_slice(&buffer[..n]);
                }
                assert!(decoded == plain, "threads {threads}, width {width}");
                // Every owner is back in the pool once the stream has ended.
                let pool = &decoder.parser.columns;
                assert_eq!(pool.spare.len(), pool.owners);
                assert_eq!(pool.owners, if threads == 1 { COLUMN_OWNERS } else { 0 });
            }
        }
    }
}
