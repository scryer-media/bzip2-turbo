#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::{self, Read, Write};

struct BoundedSink(usize);
impl Write for BoundedSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.0.min(bytes.len());
        self.0 -= count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn drain(reader: impl Read) {
    // Bound expansion work, including concatenated members.
    let _ = io::copy(&mut reader.take(1 << 20), &mut io::sink());
}

fuzz_target!(|data: &[u8]| {
    drain(bzip2_turbo::Decoder::with_threads(data, 1));
    drain(bzip2_turbo::Decoder::with_threads(data, 4));
    drain(bzip2_turbo::Decoder::with_output_callback(data, 4, |_| {}));
    drain(bzip2_turbo::Decoder::with_crc32_output_callback(
        data,
        4,
        |_, _| Ok(()),
    ));
    drain(bzip2_turbo::read::BzDecoder::new(data));
    drain(bzip2_turbo::read::MultiBzDecoder::new(data));
    drain(bzip2_turbo::bufread::BzDecoder::new(data));
    drain(bzip2_turbo::bufread::MultiBzDecoder::new(data));
    let mut writer = bzip2_turbo::write::BzDecoder::new(BoundedSink(1 << 20));
    let _ = writer.write_all(data);
    let _ = writer.try_finish();
    for small in [false, true] {
        let mut raw = bzip2_turbo::Decompress::new(small);
        let mut remaining = data;
        for _ in 0..128 {
            let before = (raw.total_in(), raw.total_out());
            let mut output = [0; 8192];
            let result = raw.decompress(remaining, &mut output);
            let consumed = (raw.total_in() - before.0) as usize;
            assert!(consumed <= remaining.len());
            assert!(raw.total_out() - before.1 <= output.len() as u64);
            remaining = &remaining[consumed..];
            if result != Ok(bzip2_turbo::Status::Ok) || before == (raw.total_in(), raw.total_out())
            {
                break;
            }
        }
    }
});
