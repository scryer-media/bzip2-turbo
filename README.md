# bzip2-turbo

Streaming BZip2 compression and decompression in Rust, translated from the
7-Zip codec. It offers the familiar `bzip2` crate API as a drop-in adapter
and a native API with explicit threading and memory settings. CRC-32/BZIP2
comes from `crc-turbo`. No libbz2 or C build is required.

With a budget of N threads, encoding runs N block workers (the calling thread
is one of them) with up to four blocks per thread in flight, and writes blocks
in input order. Decoding runs N block workers: the calling thread finds block
boundaries and delivers output in order while the workers decode blocks. The native
encoder reproduces 7-Zip's bytes for matching settings; compressed bytes can
differ from libbz2. The crate documentation lists every behavioral
difference from the `bzip2` crate.

## Usage

Replace the `bzip2` dependency and keep the `bzip2::...` imports:

```toml
[dependencies]
bzip2 = { package = "bzip2-turbo", version = "0.2" }
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

Measured on one Apple M5 Max (18 cores, macOS 26.6) on 2026-11-08 against
lbzip2 (source revision 724352c). Other work was running (load average 7 to
20), so treat gaps under about 5% as noise. Each cell is the median wall time
in seconds of five interleaved runs; lower is better. Inputs are a 256 MiB
JSON corpus and 96 MiB of concatenated build binaries. bzip2-turbo is the
`codec-bench` example built with `cargo build --release` and LTO off.

Encoding, level 9, one pass (`passes: Some(1)`), against `lbzip2 -9`:

| Seconds | 1T | 2T | 4T | 8T | 16T |
|---|---:|---:|---:|---:|---:|
| JSON, bzip2-turbo | **6.00** | **3.03** | **1.58** | **0.86** | **0.63** |
| JSON, lbzip2 | 8.68 | 4.29 | 2.25 | 1.17 | 0.91 |
| Binaries, bzip2-turbo | 2.98 | 1.53 | 0.79 | 0.44 | 0.32 |
| Binaries, lbzip2 | **2.89** | **1.46** | **0.76** | **0.40** | **0.29** |

Encoding, level 1, against `lbzip2 -1`:

| Seconds | 1T | 2T | 4T | 8T | 16T |
|---|---:|---:|---:|---:|---:|
| JSON, bzip2-turbo | **6.09** | **3.14** | **2.00** | **0.90** | **0.50** |
| JSON, lbzip2 | 7.89 | 4.03 | 2.06 | 1.05 | 0.56 |
| Binaries, bzip2-turbo | **2.88** | **1.52** | **0.80** | 0.43 | 0.23 |
| Binaries, lbzip2 | 3.11 | 1.58 | 0.81 | **0.42** | **0.22** |

Level 9's default of two passes also runs 7-Zip's Huffman table
optimisation. It takes 23.80 s (JSON) and 11.25 s (binaries) on one thread
and 2.22 s and 1.14 s on 16, about four times `lbzip2 -9`, for smaller
output:

| Bytes | Level 9 (2 passes) | 7 passes | `lbzip2 -9` | `bzip2 -9` |
|---|---:|---:|---:|---:|
| JSON | 37,919,682 | 37,919,682 | 38,092,338 | 37,924,937 |
| Binaries | 40,118,359 | 39,501,755 | 40,423,634 | 40,496,770 |

Decoding a bzip2-turbo level-9 stream:

| Seconds | 1T | 2T | 4T | 8T | 16T |
|---|---:|---:|---:|---:|---:|
| JSON, bzip2-turbo | **2.96** | **2.05** | **1.06** | **0.89** | **0.79** |
| JSON, lbzip2 | 3.36 | 2.06 | 1.23 | 0.97 | 0.82 |
| Binaries, bzip2-turbo | **1.13** | **0.62** | **0.31** | **0.18** | **0.11** |
| Binaries, lbzip2 | 1.27 | 0.69 | 0.35 | 0.21 | 0.12 |

- bzip2-turbo's thread budget includes the calling thread, which also reads
  input and writes output. `lbzip2 -n N` runs N compression threads plus
  separate input and output threads.
- The [benchmark notes](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmark.md),
  [how to reproduce them](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmarking.md)
  and the [decoder compared with 7zz](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/decoder-performance.md)
  were measured on 0.1 on other hardware and describe its two-thread
  decoder and seven-pass level 9.

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
