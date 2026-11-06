# bzip2-turbo

Streaming BZip2 compression and decompression using a Rust port of the 7-Zip
codec. Includes familiar `bzip2`-compatible adapters and explicit native APIs
for multithreading and memory policy. CRC-32/BZIP2 uses `crc-turbo`.

## Familiar streaming API

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

The examples use the Cargo rename above, so imports remain `bzip2::...`.
Without renaming the dependency, use `bzip2_turbo::...` instead. The adapter surface targets bzip2
0.6.1: `read`, `write`, `bufread`, `Compression`, `Compress`, `Decompress`,
`Action`, `Status`, and `Error`. Ordinary `BzDecoder` reads one member;
`MultiBzDecoder` reads concatenated members. Use `bufread` when bytes after the
compressed member must remain available in the underlying reader.

Compatibility covers the source interface and stream behavior, with explicit
backend differences: `work_factor` is validated but does not tune the 7-Zip
sorter, compact decoding uses a different memory representation, and compressed
bytes can differ from libbz2. Read the [compatibility contract](https://github.com/scryer-media/bzip2-turbo/blob/main/PORTING.md)
before relying on backend-specific behavior. Existing backend/linkage Cargo
features are not carried over. The default `compress` feature enables encoding;
`default-features = false` provides decoding only. The library requires `std`.

## Explicit 7-Zip settings

```rust
# use bzip2_turbo as bzip2;
use bzip2::{Encoder, EncoderOptions};
use std::io::Write;

let options = EncoderOptions { threads: 4, level: 5, ..Default::default() };
let mut encoder = Encoder::new(Vec::new(), options);
encoder.write_all(b"explicit worker budget")?;
let compressed = encoder.finish()?;
# assert!(!compressed.is_empty());
# Ok::<(), std::io::Error>(())
```

Native `Encoder` and `Decoder` default to one thread. Encoding preserves output
order across bounded worker slots. Decoding uses a parser/reconstruction
pipeline when threading is requested; requesting eight threads does not imply
eight reconstruction workers. `Decoder::with_low_memory` selects a compact inverse-BWT
lookup table. Memory estimates cover codec allocations, not whole-process RSS.
Native `EncoderOptions` follows 7-Zip's level/pass mapping, which differs from
compatibility `Compression` levels. Native `Decoder` defaults to concatenated
streams; `set_multistream(false)` selects one member.

## Validation and measurements

The [porting notes](https://github.com/scryer-media/bzip2-turbo/blob/main/PORTING.md) map behavior and tests. The
[validation record](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/validation.md) separates completed local checks from
CI coverage. [Benchmarking](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmarking.md) compares 7zz, turbo, and isolated standard Rust bzip2 pure Rust/C backends with
verified output, alternating rounds, medians and RSS.

## Benchmarks

Level 5, all fixtures combined (JSON, logs, repeated text, MKV, ELF). Higher is
better. 1T–8T is the requested thread count.

### ARM64 (Graviton3)

| Engine | Encode 1T (MiB/s) | Encode 2T (MiB/s) | Encode 4T (MiB/s) | Encode 8T (MiB/s) | Decode 1T (MiB/s) | Decode 2T+ (MiB/s) |
|---|---:|---:|---:|---:|---:|---:|
| **bzip2-turbo** | **10.6** | **18.0** | **32.1** | **54.0** | **47.0** | **66.0** |
| 7zz | 6.8 | 12.0 | 21.7 | 38.1 | 46.4 | 64.8 |
| bzip2 (Rust) | 8.9 | — | — | — | 34.4 | — |
| bzip2 (C) | 8.5 | — | — | — | 35.7 | — |

### x86-64 (Intel Xeon)

| Engine | Encode 1T (MiB/s) | Encode 2T (MiB/s) | Encode 4T (MiB/s) | Encode 8T (MiB/s) | Decode 1T (MiB/s) | Decode 2T+ (MiB/s) |
|---|---:|---:|---:|---:|---:|---:|
| **bzip2-turbo** | **13.0** | **22.3** | **39.9** | **45.9** | **84.6** | **117.5** |
| 7zz | 9.3 | 16.8 | 30.2 | 38.0 | 71.5 | 90.0 |
| bzip2 (Rust) | 11.0 | — | — | — | 42.0 | — |
| bzip2 (C) | 9.8 | — | — | — | 42.5 | — |

- Neither engine decodes faster past two threads (bzip2-turbo uses at most
  two), so the 2T figure, the best of 2T, 4T and 8T, is shown.
- The standard `bzip2` crate is single-threaded.
- These are combined totals; bzip2-turbo does not lead on every fixture.
- [Per-fixture results, all levels, peak RSS and method](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmark.md).

## License

The translated codec is LGPL-2.1-or-later: [COPYING](https://github.com/scryer-media/bzip2-turbo/blob/main/COPYING) and
[7-Zip notices](https://github.com/scryer-media/bzip2-turbo/blob/main/LICENSE-7ZIP). The compatibility adapters and API definitions
retain [MIT notices](https://github.com/scryer-media/bzip2-turbo/blob/main/LICENSE-BZIP2-MIT). Package metadata declares both.
No libbz2 runtime or C codec build is required by the library. Standard bzip2
is a dev-only oracle; optional standalone benchmark tools can build either its
pure Rust or bundled C backend. See the [publishing prerequisites](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/publishing.md)
for the operator-owned first publication and downstream integration steps.
