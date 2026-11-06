# bzip2-turbo

Streaming BZip2 compression and decompression in Rust, translated from the
7-Zip codec. It offers the familiar `bzip2` crate API as a drop-in adapter
and a native API with explicit threading and memory settings. CRC-32/BZIP2
comes from `crc-turbo`. No libbz2 or C build is required.

Encoding runs across bounded worker slots with output order preserved.
Decoding pipelines parsing and reconstruction across two threads. The native
encoder reproduces 7-Zip's bytes for matching settings; compressed bytes can
differ from libbz2. The crate documentation lists every behavioral
difference from the `bzip2` crate.

## Usage

Replace the `bzip2` dependency and keep the `bzip2::...` imports:

```toml
[dependencies]
bzip2 = { package = "bzip2-turbo", version = "0.1" }
```

```rust
# use bzip2_turbo as bzip2;
use bzip2::{Compression, read::BzDecoder, write::BzEncoder};
use std::io::{Read, Write};

let mut encoder = BzEncoder::new(Vec::new(), Compression::default());
encoder.write_all(b"a streaming codec")?;
let compressed = encoder.finish()?;
let mut decoder = BzDecoder::new(compressed.as_slice());
let mut restored = Vec::new();
decoder.read_to_end(&mut restored)?;
assert_eq!(restored, b"a streaming codec");
# Ok::<(), std::io::Error>(())
```

The native API exposes 7-Zip's own settings:

```rust
# use bzip2_turbo as bzip2;
use bzip2::{Decoder, Encoder, EncoderOptions};
use std::io::{Read, Write};

let options = EncoderOptions { threads: 4, level: 5, ..Default::default() };
let mut encoder = Encoder::new(Vec::new(), options);
encoder.write_all(b"explicit worker budget")?;
let compressed = encoder.finish()?;
let mut restored = Vec::new();
Decoder::new(compressed.as_slice()).read_to_end(&mut restored)?;
assert_eq!(restored, b"explicit worker budget");
# Ok::<(), std::io::Error>(())
```

The default `compress` feature enables encoding; `default-features = false`
leaves decoding only. The library requires `std`. API details, memory policy
and the decoder's design are in the crate documentation and under
[docs](https://github.com/scryer-media/bzip2-turbo/tree/main/docs).

## Benchmarks

Level-5 combined-corpus throughput in MiB/s: total original bytes divided by
the sum of each fixture's median wall time over four measured rounds, using
varied JSON, structured logs, repeated text, MKV and ELF. Whole-process
measurements with a null output sink; budgets are requested thread counts.
Native decoding uses at most two threads, so decode is measured at 1T and 2T.
The pure Rust and C references are serial.

### ARM64 — Graviton3

| Engine / budget | Encode | Decode |
|---|---:|---:|
| bzip2-turbo / 1T | 10.6 | 47.0 |
| 7zz / 1T | 6.8 | 46.4 |
| bzip2 (Rust) / 1T | 8.9 | 34.4 |
| bzip2 (C) / 1T | 8.5 | 35.7 |
| bzip2-turbo / 2T | 18.0 | 66.0 |
| 7zz / 2T | 12.0 | 64.8 |
| bzip2-turbo / 4T | 32.1 | — |
| 7zz / 4T | 21.7 | — |
| bzip2-turbo / 8T | 54.0 | — |
| 7zz / 8T | 38.1 | — |

### x86-64 — Intel Xeon

| Engine / budget | Encode | Decode |
|---|---:|---:|
| bzip2-turbo / 1T | 13.0 | 84.6 |
| 7zz / 1T | 9.3 | 71.5 |
| bzip2 (Rust) / 1T | 11.0 | 42.0 |
| bzip2 (C) / 1T | 9.8 | 42.5 |
| bzip2-turbo / 2T | 22.3 | 117.5 |
| 7zz / 2T | 16.8 | 90.0 |
| bzip2-turbo / 4T | 39.9 | — |
| 7zz / 4T | 30.2 | — |
| bzip2-turbo / 8T | 45.9 | — |
| 7zz / 8T | 38.0 | — |

Aggregate rates do not imply a lead on every fixture. Every fixture, level,
budget, peak RSS and source identity is in the
[detailed results](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmark.md);
the [benchmarking guide](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmarking.md)
reproduces them and the
[decoder performance notes](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/decoder-performance.md)
compare the native decoder with 7zz case by case.

## Attribution

- The codec is a translation of the BZip2 encoder and decoder in
  [7-Zip](https://www.7-zip.org/) by Igor Pavlov, pinned to the 26.04
  [source revision](https://github.com/ip7z/7zip/tree/9128b80e3a471108678e2b3ec3c985861c8d7b0f),
  together with its public-domain block sorting and Huffman helpers.
- The streaming adapters and API definitions are reused from the Rust
  [`bzip2`](https://crates.io/crates/bzip2) crate, version 0.6.1, by Alex
  Crichton and contributors.
- Checksums use [`crc-turbo`](https://crates.io/crates/crc-turbo).

bzip2-turbo is an independent project and is not affiliated with or endorsed
by 7-Zip or the `bzip2` crate.

## License

The translated codec is LGPL-2.1-or-later: see
[COPYING](https://github.com/scryer-media/bzip2-turbo/blob/main/COPYING) and the
[7-Zip notices](https://github.com/scryer-media/bzip2-turbo/blob/main/LICENSE-7ZIP).
The compatibility adapters and API definitions retain their
[MIT notices](https://github.com/scryer-media/bzip2-turbo/blob/main/LICENSE-BZIP2-MIT).
Package metadata declares both.

---

Part of [scryer-media](https://github.com/scryer-media). Issues and security
reports: see [SECURITY.md](https://github.com/scryer-media/bzip2-turbo/blob/main/SECURITY.md).
