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
On Linux x86-64 and AArch64 the streaming `Decoder`'s lookup tables and last
columns are whole 2 MiB pages advised for transparent huge pages, which the
estimates count; the bzip2-compatible API (`Decompress`, the `read`, `bufread`
and `write` decoders and `MultiBzDecoder`) gets the huge-page lookup table only
and keeps each last column in a plain `Vec`. These pages are mapped directly
with `mmap`, independent of the process allocator (Rust's global allocator or
the linked C `malloc`), and the kernel zeroes them on first touch; it uses huge
pages only when that setting is `madvise` or `always`.
Native `EncoderOptions` follows 7-Zip's level/pass mapping, which differs from
compatibility `Compression` levels. Native `Decoder` defaults to concatenated
streams; `set_multistream(false)` selects one member.

## Validation and measurements

The [porting notes](https://github.com/scryer-media/bzip2-turbo/blob/main/PORTING.md) map behavior and tests. The
[validation record](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/validation.md) separates completed local checks from
CI coverage. [Benchmarking](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmarking.md) compares 7zz, turbo, and isolated standard Rust bzip2 pure Rust/C backends with
verified output, alternating rounds, medians and RSS.

## Benchmarks

Level-5 combined-corpus throughput in MiB/s: total original bytes divided by the sum of each fixture's median wall time. Four balanced measured rounds follow one warmup, using varied JSON, structured logs, repeated text, MKV and ELF. These are whole-process measurements with a uniform null output sink.

### ARM64 — Graviton3

| Engine / budget | Encode | Decode |
|---|---:|---:|
| bzip2-turbo / 1T | 10.6 | 47.0 |
| 7zz / 1T | 6.8 | 46.4 |
| bzip2 (Rust) / 1T | 8.9 | 34.4 |
| bzip2 (C) / 1T | 8.5 | 35.7 |
| bzip2-turbo / 2T | 18.0 | 66.0 |
| 7zz / 2T | 12.0 | 64.8 |
| bzip2-turbo / 4T | 32.1 | 65.7 |
| 7zz / 4T | 21.7 | 64.7 |
| bzip2-turbo / 8T | 54.0 | 65.0 |
| 7zz / 8T | 38.1 | 64.5 |

### x86-64 — Intel Xeon

| Engine / budget | Encode | Decode |
|---|---:|---:|
| bzip2-turbo / 1T | 13.0 | 84.6 |
| 7zz / 1T | 9.3 | 71.5 |
| bzip2 (Rust) / 1T | 11.0 | 42.0 |
| bzip2 (C) / 1T | 9.8 | 42.5 |
| bzip2-turbo / 2T | 22.3 | 117.5 |
| 7zz / 2T | 16.8 | 90.0 |
| bzip2-turbo / 4T | 39.9 | 112.2 |
| 7zz / 4T | 30.2 | 88.9 |
| bzip2-turbo / 8T | 45.9 | 116.5 |
| 7zz / 8T | 38.0 | 89.1 |

Budgets are requested thread counts. Native bzip2-turbo decoding uses at most two threads including the caller; its 4T/8T rows retain those requests. The pure Rust and C references are serial and appear once at 1T. See [detailed results and methodology](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/benchmark.md) for every fixture, level, budget, peak RSS and source identity.

Aggregate rates do not imply a lead on every fixture.

## License

The translated codec is LGPL-2.1-or-later: [COPYING](https://github.com/scryer-media/bzip2-turbo/blob/main/COPYING) and
[7-Zip notices](https://github.com/scryer-media/bzip2-turbo/blob/main/LICENSE-7ZIP). The compatibility adapters and API definitions
retain [MIT notices](https://github.com/scryer-media/bzip2-turbo/blob/main/LICENSE-BZIP2-MIT). Package metadata declares both.
No libbz2 runtime or C codec build is required by the library. Standard bzip2
is a dev-only oracle; optional standalone benchmark tools can build either its
pure Rust or bundled C backend. See the [publishing prerequisites](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/publishing.md)
for the operator-owned first publication and downstream integration steps.
