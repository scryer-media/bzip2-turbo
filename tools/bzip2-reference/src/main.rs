//! Standalone standard bzip2 API driver with mutually exclusive backends.
#[cfg(any(
    all(feature = "pure", feature = "c"),
    not(any(feature = "pure", feature = "c"))
))]
compile_error!("select exactly one backend: pure or c");

use bzip2::{Compression, read, write};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};

fn main() -> io::Result<()> {
    let mut args: Vec<_> = std::env::args().collect();
    if args.len() == 2 && matches!(args[1].as_str(), "--version" | "--identity") {
        let backend = if cfg!(feature = "pure") {
            "pure (libbz2-rs-sys)"
        } else {
            "c (bzip2-sys, bundled static)"
        };
        println!(
            "bzip2-reference-bench {} bzip2=0.6.1 backend={backend}",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.get(1).is_some_and(|arg| arg == "--api") {
        if args.get(2).map(String::as_str) != Some("compat") {
            return Err(io::Error::other("reference API must be compat"));
        }
        args.drain(1..3);
    }
    if !(6..=8).contains(&args.len()) {
        return Err(io::Error::other(
            "usage: bzip2-reference-bench [--api compat] encode|decode LEVEL THREADS INPUT OUTPUT|- [BLOCK_SIZE|-] [PASSES|-]",
        ));
    }
    let level: u32 = args[2]
        .parse()
        .map_err(|_| io::Error::other("invalid level"))?;
    let threads: usize = args[3]
        .parse()
        .map_err(|_| io::Error::other("invalid threads"))?;
    if !(1..=9).contains(&level) || threads != 1 {
        return Err(io::Error::other(
            "level must be 1..9 and reference requires one thread",
        ));
    }
    if args.iter().skip(6).any(|value| value != "-") {
        return Err(io::Error::other(
            "reference requires default block/pass options",
        ));
    }
    if !matches!(args[1].as_str(), "encode" | "decode") {
        return Err(io::Error::other("mode must be encode or decode"));
    }
    let mut input = BufReader::new(File::open(&args[4])?);
    let output: Box<dyn Write> = if args[5] == "-" {
        Box::new(io::sink())
    } else {
        Box::new(BufWriter::new(File::create(&args[5])?))
    };
    match args[1].as_str() {
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
        _ => unreachable!(),
    }
}
