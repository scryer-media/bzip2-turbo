#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::{Read, Write};

fuzz_target!(|data: &[u8]| {
    if data.len() > 16384 {
        return;
    }
    let mut encoder = bzip2_turbo::Encoder::new(
        Vec::new(),
        bzip2_turbo::EncoderOptions {
            level: 1,
            threads: 1,
            ..Default::default()
        },
    );
    encoder.write_all(data).unwrap();
    let packed = encoder.finish().unwrap();
    let mut plain = Vec::new();
    bzip2_turbo::Decoder::with_threads(packed.as_slice(), 1)
        .take(data.len() as u64 + 1)
        .read_to_end(&mut plain)
        .unwrap();
    assert_eq!(plain, data);
    let mut packed_read = Vec::new();
    bzip2_turbo::read::BzEncoder::new(data, bzip2_turbo::Compression::fast())
        .read_to_end(&mut packed_read)
        .unwrap();
    let mut packed_bufread = Vec::new();
    bzip2_turbo::bufread::BzEncoder::new(data, bzip2_turbo::Compression::fast())
        .read_to_end(&mut packed_bufread)
        .unwrap();
    let mut writer =
        bzip2_turbo::write::BzEncoder::new(Vec::new(), bzip2_turbo::Compression::fast());
    writer.write_all(data).unwrap();
    let packed_write = writer.finish().unwrap();
    assert_eq!(packed_read, packed_bufread);
    assert_eq!(packed_read, packed_write);
    let mut plain = Vec::new();
    bzip2_turbo::read::BzDecoder::new(packed_read.as_slice())
        .take(data.len() as u64 + 1)
        .read_to_end(&mut plain)
        .unwrap();
    assert_eq!(plain, data);
});
