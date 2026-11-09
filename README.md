# bzip2-turbo

Streaming BZip2 compression and decompression in Rust, translated from the
7-Zip codec. It offers the familiar `bzip2` crate API as a drop-in adapter
and a native API with explicit threading and memory settings. CRC-32/BZIP2
comes from `crc-turbo`. No libbz2 or C build is required.

With N threads above one, encoding runs N compression workers with up to four
blocks per worker in flight, while the calling thread reads input, run-length
encodes it into blocks and writes the encoded blocks in input order. Decoding
runs N block workers while the calling thread finds block boundaries and
delivers output in order. One thread encodes or decodes on the calling
thread alone. The native encoder reproduces 7-Zip's bytes for matching
settings (7-Zip's level 9 is `passes: Some(7)`); compressed bytes can differ
from libbz2. The crate documentation lists every behavioral difference from
the `bzip2` crate.

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
lbzip2 (source revision 724352c). Other work was running (load average 5 to
14), so treat gaps under about 5% as noise. Each cell is the median wall time
in seconds of five interleaved runs; lower is better. Inputs are a 256 MiB
JSON corpus and 96 MiB of concatenated build binaries. bzip2-turbo is the
`codec-bench` example built with `cargo build --release` and LTO off. Its
`threads: N` and `lbzip2 -n N` both mean N compression threads, plus the
threads that read and write.

Encoding, level 9 (one pass, the default), against `lbzip2 -9`:

| Seconds | 1T | 2T | 4T | 8T | 16T |
|---|---:|---:|---:|---:|---:|
| JSON, bzip2-turbo | **5.82** | **2.85** | **1.52** | **0.84** | **0.50** |
| JSON, lbzip2 | 8.37 | 4.25 | 2.20 | 1.15 | 0.67 |
| Binaries, bzip2-turbo | **2.82** | **1.41** | **0.74** | 0.41 | 0.25 |
| Binaries, lbzip2 | 2.93 | 1.44 | 0.75 | **0.40** | **0.23** |

A recheck of the binaries at 8 and 16 threads with eleven runs gave 0.39 s
for both at 8 threads, and 0.23 s against lbzip2's 0.21 s at 16. Peak RSS at
16 threads on the binaries is 238 MB against lbzip2's 136 MB. At level 1
against `lbzip2 -1`, bzip2-turbo is 19 to 25% faster on the JSON and 5 to 8%
faster on the binaries at every thread count.

More passes (`passes: Some(2)` or more) also run 7-Zip's Huffman table
optimisation, which is several times slower; those timings were not
re-measured here. Sizes at level 9:

| Bytes | 1 pass (default) | 2 passes | 7 passes | `lbzip2 -9` | `bzip2 -9` |
|---|---:|---:|---:|---:|---:|
| JSON | 37,924,470 | 37,919,682 | 37,919,682 | 38,092,338 | 37,924,937 |
| Binaries | 40,497,139 | 40,146,131 | 39,575,538 | 40,326,306 | 40,499,587 |

Decoding a bzip2-turbo level-9 stream:

| Seconds | 1T | 2T | 4T | 8T | 16T |
|---|---:|---:|---:|---:|---:|
| JSON, bzip2-turbo | 4.71 | 3.76 | **2.18** | **1.39** | 0.87 |
| JSON, lbzip2 | **3.60** | **3.46** | 2.35 | 1.40 | **0.69** |
| Binaries, bzip2-turbo | **1.32** | **0.82** | **0.43** | **0.24** | **0.12** |
| Binaries, lbzip2 | 1.35 | 0.89 | 0.49 | 0.27 | 0.14 |

The JSON decode cells at 1 and 16 threads were unstable on this busy
machine: bzip2-turbo's single runs at one thread ranged from 3.85 to 6.74 s,
and a nine-run recheck at 16 threads gave 0.54 s against lbzip2's 0.51 s.

The [benchmark notes](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmark.md)
compare 0.2.0 with 7zz and the Rust and C bzip2 crates on AWS Graviton3 and
Intel hosts at levels 1, 5 and 9 and 1 to 8 threads
([how to reproduce them](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmarking.md)).
The [decoder compared with 7zz](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/decoder-performance.md)
holds older results for 0.1 and its two-thread decoder.

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
