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

Level 5, all fixtures combined (JSON, logs, repeated text, MKV, ELF). Higher is
better. 1T–8T is the requested thread count.

### ARM64 (Graviton3)

| Encode MiB/s | 1T | 2T | 4T | 8T |
|---|---:|---:|---:|---:|
| **bzip2-turbo** | **10.6** | **18.0** | **32.1** | **54.0** |
| 7zz | 6.8 | 12.0 | 21.7 | 38.1 |
| bzip2 (Rust) | 8.9 | 8.9 | 8.9 | 8.9 |
| bzip2 (C) | 8.5 | 8.5 | 8.4 | 8.4 |

| Decode MiB/s | 1T | 2T |
|---|---:|---:|
| **bzip2-turbo** | **47.0** | **66.0** |
| 7zz | 46.4 | 64.8 |
| bzip2 (Rust) | 34.4 | 34.5 |
| bzip2 (C) | 35.7 | 35.7 |

### x86-64 (Intel Xeon)

| Encode MiB/s | 1T | 2T | 4T | 8T |
|---|---:|---:|---:|---:|
| **bzip2-turbo** | **13.0** | **22.3** | **39.9** | **45.9** |
| 7zz | 9.3 | 16.8 | 30.2 | 38.0 |
| bzip2 (Rust) | 11.0 | 10.9 | 10.9 | 10.8 |
| bzip2 (C) | 9.8 | 9.7 | 9.7 | 9.7 |

| Decode MiB/s | 1T | 2T |
|---|---:|---:|
| **bzip2-turbo** | **84.6** | **117.5** |
| 7zz | 71.5 | 90.0 |
| bzip2 (Rust) | 42.0 | 42.1 |
| bzip2 (C) | 42.5 | 42.7 |

- Decoding uses at most two threads, so it is measured at 1T and 2T only.
- The standard `bzip2` crate is single-threaded, so its rate stays flat as
  threads are added: at 2T bzip2-turbo encodes 2x and decodes 1.9x (ARM64)
  to 2.8x (x86-64) faster.
- These are combined totals; bzip2-turbo does not lead on every fixture.
- [Per-fixture results, all levels and peak RSS](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmark.md),
  [how to reproduce them](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmarking.md),
  and the [decoder compared with 7zz case by case](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/decoder-performance.md).

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
