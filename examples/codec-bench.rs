//! Whole-process benchmark driver for native and compatibility stream APIs.
//!
//! Usage: `codec-bench [--api native|compat] encode|decode LEVEL THREADS INPUT
//! OUTPUT|- [BLOCK_SIZE|-] [PASSES|-]`. `-` discards output; it is not stdout.
//! Input is always a file. Time the entire process externally, including I/O,
//! setup and finalization, and compare decoded bytes in a separate check.
//!
//! Native encoding accepts block/pass overrides. Decoding uses stream settings,
//! so level/block/pass arguments do not configure the decoder. Compatibility
//! mode requires one thread and no overrides; its decoder stops at one member.
//! Native decoder thread budgets above two still use only one reconstruction
//! worker. The driver validates levels `1..=9` even when decoding.
use bzip2_turbo::{Compression, Decoder, Encoder, EncoderOptions, read, write};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};

fn main() -> io::Result<()> {
    let mut args: Vec<_> = std::env::args().collect();
    let api = if args.get(1).is_some_and(|arg| arg == "--api") {
        if args.len() < 4 {
            return Err(io::Error::other("--api requires native or compat"));
        }
        let api = args.remove(2);
        args.remove(1);
        api
    } else {
        "native".to_owned()
    };
    if !matches!(api.as_str(), "native" | "compat") {
        return Err(io::Error::other("--api must be native or compat"));
    }
    if !(6..=8).contains(&args.len()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: codec-bench [--api native|compat] encode|decode LEVEL THREADS INPUT OUTPUT|- [BLOCK_SIZE|-] [PASSES|-]",
        ));
    }
    let level: u32 = args[2]
        .parse()
        .map_err(|_| io::Error::other("invalid level"))?;
    let threads: usize = args[3]
        .parse()
        .map_err(|_| io::Error::other("invalid threads"))?;
    let optional = |index: usize| -> io::Result<Option<u32>> {
        args.get(index)
            .filter(|value| value.as_str() != "-")
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| io::Error::other("invalid codec option"))
            })
            .transpose()
    };
    let block_size = optional(6)?;
    let passes = optional(7)?;
    if !(1..=9).contains(&level) || threads == 0 {
        return Err(io::Error::other(
            "level must be 1..9 and threads must be positive",
        ));
    }
    if api == "compat" && (threads != 1 || block_size.is_some() || passes.is_some()) {
        return Err(io::Error::other(
            "compat API requires one thread and default block/pass options",
        ));
    }
    let mut input = BufReader::new(File::open(&args[4])?);
    let output: Box<dyn Write> = if args[5] == "-" {
        Box::new(io::sink())
    } else {
        Box::new(BufWriter::new(File::create(&args[5])?))
    };
    if api == "compat" {
        return match args[1].as_str() {
            "encode" => {
                let mut encoder = write::BzEncoder::new(output, Compression::new(level));
                io::copy(&mut input, &mut encoder)?;
                encoder.finish()?.flush()
            }
            "decode" => {
                let mut decoder = read::BzDecoder::new(input);
                let mut output = output;
                io::copy(&mut decoder, &mut output)?;
                output.flush()
            }
            _ => Err(io::Error::other("mode must be encode or decode")),
        };
    }
    match args[1].as_str() {
        "encode" => {
            let mut encoder = Encoder::new(
                output,
                EncoderOptions {
                    level,
                    threads,
                    block_size,
                    passes,
                },
            );
            io::copy(&mut input, &mut encoder)?;
            encoder.finish()?.flush()
        }
        "decode" => {
            let mut decoder = Decoder::with_threads(input, threads);
            let mut output = output;
            io::copy(&mut decoder, &mut output)?;
            output.flush()
        }
        _ => Err(io::Error::other("mode must be encode or decode")),
    }
}
