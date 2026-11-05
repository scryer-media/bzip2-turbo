#![cfg(feature = "compress")]

use bzip2_turbo::{Action, Compress, Compression, Decompress, Status, bufread, read, write};
use std::io::{self, BufRead, BufReader, Read, Write};

fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = write::BzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).unwrap();
    assert_eq!(encoder.total_in(), bytes.len() as u64);
    encoder.finish().unwrap()
}

#[test]
fn readers_and_writers_roundtrip_empty_and_block_boundaries() {
    for size in [0, 1, 8193, 100_031] {
        let input: Vec<u8> = (0..size).map(|n| (n * 157 + n / 17) as u8).collect();
        let packed = compressed(&input);
        let mut reader = read::BzDecoder::new(&packed[..]);
        let mut decoded = Vec::new();
        reader.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, input);
        assert_eq!(reader.total_out(), size as u64);
        let mut writer = write::BzDecoder::new(Vec::new());
        for chunk in packed.chunks(7) {
            writer.write_all(chunk).unwrap();
        }
        assert_eq!(writer.finish().unwrap(), input);
        let mut encoder = read::BzEncoder::new(&input[..], Compression::fast());
        let mut packed_reader = Vec::new();
        encoder.read_to_end(&mut packed_reader).unwrap();
        assert_eq!(packed_reader, packed);
    }
}

#[test]
fn buffered_decoder_preserves_tail_and_multistream_decodes_members() {
    let first = compressed(b"first");
    let second = compressed(b"second");
    let joined = [first.as_slice(), second.as_slice()].concat();
    let mut decoder = bufread::BzDecoder::new(BufReader::new(joined.as_slice()));
    let mut result = Vec::new();
    decoder.read_to_end(&mut result).unwrap();
    assert_eq!(result, b"first");
    assert_eq!(decoder.total_in(), first.len() as u64);
    let mut remainder = decoder.into_inner();
    assert_eq!(remainder.fill_buf().unwrap(), second);
    result.clear();
    read::MultiBzDecoder::new(joined.as_slice())
        .read_to_end(&mut result)
        .unwrap();
    assert_eq!(result, b"firstsecond");
}

#[test]
fn truncation_and_invalid_input_report_errors() {
    let packed = compressed(b"hostile truncation checks");
    for cut in 0..packed.len() {
        let mut decoder = read::BzDecoder::new(&packed[..cut]);
        let error = decoder.read_to_end(&mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "cut {cut}");
    }
    let mut decoder = write::BzDecoder::new(Vec::new());
    assert_eq!(
        decoder.write(b"BZh\xfb").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    let mut decoder = write::BzDecoder::new(Vec::new());
    assert_eq!(
        decoder.try_finish().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[derive(Default)]
struct HostileSink {
    bytes: Vec<u8>,
    block: bool,
    zero: bool,
}
impl Write for HostileSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.block {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        if self.zero {
            return Ok(0);
        }
        let count = bytes.len().min(3);
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn finish_retries_would_block_and_zero_writes_fail() {
    let mut encoder = write::BzEncoder::new(HostileSink::default(), Compression::fast());
    encoder.write_all(b"short writes and backpressure").unwrap();
    encoder.get_mut().block = true;
    assert_eq!(
        encoder.try_finish().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    encoder.get_mut().block = false;
    encoder.try_finish().unwrap();
    encoder.try_finish().unwrap();
    let sink = encoder.finish().unwrap();
    let mut result = Vec::new();
    read::BzDecoder::new(sink.bytes.as_slice())
        .read_to_end(&mut result)
        .unwrap();
    assert_eq!(result, b"short writes and backpressure");
    let mut encoder = write::BzEncoder::new(
        HostileSink {
            zero: true,
            ..Default::default()
        },
        Compression::fast(),
    );
    encoder.write_all(b"zero write").unwrap();
    assert_eq!(
        encoder.try_finish().unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
}

#[test]
fn flush_continues_stream_and_drop_finalizes_it() {
    let mut packed = Vec::new();
    {
        let mut encoder = write::BzEncoder::new(&mut packed, Compression::fast());
        encoder.write_all(b"before").unwrap();
        encoder.flush().unwrap();
        encoder.write_all(b"after").unwrap();
    }
    let mut result = Vec::new();
    read::BzDecoder::new(packed.as_slice())
        .read_to_end(&mut result)
        .unwrap();
    assert_eq!(result, b"beforeafter");
}

#[test]
fn raw_decoder_stops_at_stream_end_with_trailing_input() {
    let packed = compressed(b"raw counters");
    let joined = [packed.as_slice(), b"tail"].concat();
    let mut decoder = Decompress::new(false);
    let mut output = [0; 64];
    assert_eq!(
        decoder.decompress(&joined, &mut output).unwrap(),
        Status::StreamEnd
    );
    assert_eq!(decoder.total_in(), packed.len() as u64);
    assert_eq!(&output[..decoder.total_out() as usize], b"raw counters");
    let mut encoder = Compress::new(Compression::fast(), 30);
    let mut output = Vec::with_capacity(1024);
    assert_eq!(
        encoder
            .compress_vec(b"raw counters", &mut output, Action::Run)
            .unwrap(),
        Status::RunOk
    );
    while encoder
        .compress_vec(&[], &mut output, Action::Finish)
        .unwrap()
        != Status::StreamEnd
    {}
    assert_eq!(output, packed);
}

#[test]
fn upstream_oracle_decodes_native_and_native_decodes_upstream() {
    let input: Vec<u8> = (0..110_003).map(|n| (n * 79 + n / 31) as u8).collect();
    let native = compressed(&input);
    let mut decoded = Vec::new();
    bzip2::read::BzDecoder::new(native.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, input);
    let mut oracle = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    oracle.write_all(&input).unwrap();
    oracle.flush().unwrap();
    oracle.write_all(b"suffix").unwrap();
    let packed = oracle.finish().unwrap();
    decoded.clear();
    read::BzDecoder::new(packed.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(&decoded[..input.len()], input);
    assert_eq!(&decoded[input.len()..], b"suffix");
    for cut in [0, 1, 4, packed.len() - 1] {
        let native_error = read::BzDecoder::new(&packed[..cut])
            .read_to_end(&mut Vec::new())
            .unwrap_err();
        let oracle_error = bzip2::read::BzDecoder::new(&packed[..cut])
            .read_to_end(&mut Vec::new())
            .unwrap_err();
        assert_eq!(native_error.kind(), oracle_error.kind());
    }
}

struct ShortReader<'a> {
    bytes: &'a [u8],
    calls: usize,
}
impl Read for ShortReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        if self.calls.is_multiple_of(3) {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = output.len().min(self.bytes.len()).min(5);
        output[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        Ok(count)
    }
}

#[test]
fn short_reader_would_block_preserves_consumption() {
    let input = b"short reader backpressure preserves every byte";
    let packed = compressed(input);
    let source = ShortReader {
        bytes: &packed,
        calls: 0,
    };
    let mut decoder = read::BzDecoder::new(source);
    let mut result = Vec::new();
    loop {
        let mut byte = [0];
        match decoder.read(&mut byte) {
            Ok(0) => break,
            Ok(1) => result.push(byte[0]),
            Ok(_) => unreachable!(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
            Err(error) => panic!("unexpected read error: {error}"),
        }
    }
    assert_eq!(result, input);
    assert_eq!(decoder.total_in(), packed.len() as u64);
}
