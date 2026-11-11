# bzip2-turbo

Streaming BZip2 compression and decompression in Rust, translated from the
7-Zip codec. It offers the familiar `bzip2` crate API as a drop-in adapter
and a native API with explicit threading and memory settings. CRC-32/BZIP2
comes from `crc-turbo`. No libbz2 or C build is required.

Encoding and decoding can both use several threads: with `threads: N` above
one, N workers handle blocks while the calling thread reads input and writes
output in order. With matching settings the encoder writes the same bytes as
7-Zip (7-Zip's level 9 is `passes: Some(7)`); its output can differ from
libbz2's. The crate documentation lists every behavioral difference from the
`bzip2` crate.

## Usage

Replace the `bzip2` dependency and keep the `bzip2::...` imports:

```toml
[dependencies]
bzip2 = { package = "bzip2-turbo", version = "0.3" }
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

## Performance

Speed in MB/s of uncompressed data, compressing and decompressing 16.8 MB of
JSON at level 9 (900 kB blocks) on two AWS machines. Higher is better. The
`bzip2` crate is single-threaded, so it has no 8-thread figures.

Intel Xeon Platinum 8488C (c7i.2xlarge, 4 cores, 8 threads):

| MB/s | Encode 1T | Encode 8T | Decode 1T | Decode 8T |
|---|---:|---:|---:|---:|
| bzip2-turbo 0.2.0 | 25 | 97 | 43 | 170 |
| 7-Zip 26.04 | 10 | 49 | 43 | 49 |
| `bzip2`, C backend | 14 | – | 32 | – |
| `bzip2`, Rust backend | 14 | – | 33 | – |

Graviton3 (c7g.2xlarge, 8 cores):

| MB/s | Encode 1T | Encode 8T | Decode 1T | Decode 8T |
|---|---:|---:|---:|---:|
| bzip2-turbo 0.2.0 | 21 | 116 | 35 | 135 |
| 7-Zip 26.04 | 10 | 60 | 35 | 41 |
| `bzip2`, C backend | 14 | – | 28 | – |
| `bzip2`, Rust backend | 14 | – | 28 | – |

bzip2-turbo is the fastest encoder on every input, level and thread count
tested. On one thread it decodes as fast as 7-Zip; given more threads it
keeps scaling, while 7-Zip gains little past two.

The [benchmarks](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmarking.md)
cover four more inputs (logs, a Linux binary, video and repeated text),
levels 1, 5 and 9, 1 to 8 threads, memory use, and how to rerun them.

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
