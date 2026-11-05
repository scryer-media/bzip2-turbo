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

### Current Graviton decoder

Measured on 2026-11-05 on a c7g.2xlarge (Graviton3, 8 physical cores, 16 GiB RAM, Ubuntu 24.04). Single-threaded runs use one pinned core; the two-thread budget uses two pinned cores. Rust 1.97.1 uses O3, fat LTO and one codegen unit. The reference is pinned 7-Zip 26.04 with its stock GCC O2 build. Standard `bzip2` 0.6.1 uses `libbz2-rs-sys` 0.2.6 or libbz2 1.0.8.

These are native `Decoder` measurements, not compatibility-adapter timings. Turbo beats both standard bzip2 backends on these fixtures, but full parity with 7zz is **not** achieved. The remaining differences are explicit below. Positive percentages mean turbo takes longer; negative percentages mean it is faster. Differences near zero should be read alongside run-to-run variation.

| Fixture | 1 thread | 2 threads |
|---|---:|---:|
| JSON | +0.30% | +1.47% |
| Logs | -0.74% | +3.12% |
| Repeated text | -41.38% | -38.48% |
| MKV | +3.56% | +0.86% |
| ELF | +0.59% | -3.10% |

Additional 100,000-byte-block checks still show larger gaps versus 7zz:
JSON takes 12.4–16.2% longer, logs 14.8–22.8%, and single-threaded MKV 10.6%.
Across the level-1 and level-9 comparisons with the previous turbo runtime,
most cases improved, while repeated-text medians were 0.11–0.41% slower.
The [full regression comparison](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/decoder-performance.md#additional-block-size-checks) retains both wins and losses.

Final signed Graviton decode comparison, source `8a2188e6b667`. Level 5, 500,000-byte blocks, six measured runs per engine after one warmup, with execution order balanced between each pair. All engines write to the operating-system null device. Elapsed time includes GNU time startup; RSS is its reported peak for the codec process. Two-thread runs use two physical cores; the standard Rust and C backends remain single-threaded. Throughput uses original input bytes. These results are separate from the earlier ARM/Intel matrix.

### Varied JSON, thread budget 1

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 41.2 | 5.4 |
| 7zz | 41.4 | 9.7 |
| bzip2 (Rust) | 32.2 | 3.5 |
| bzip2 (C) | 31.4 | 3.5 |

### Varied JSON, thread budget 2

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 48.3 | 5.2 |
| 7zz | 49.0 | 9.8 |
| bzip2 (Rust) | 32.1 | 3.5 |
| bzip2 (C) | 31.4 | 3.5 |

### Structured logs, thread budget 1

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 66.5 | 5.4 |
| 7zz | 66.0 | 9.7 |
| bzip2 (Rust) | 50.6 | 3.5 |
| bzip2 (C) | 47.2 | 3.5 |

### Structured logs, thread budget 2

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 75.6 | 5.1 |
| 7zz | 78.0 | 9.8 |
| bzip2 (Rust) | 50.2 | 3.5 |
| bzip2 (C) | 47.0 | 3.5 |

### Repeated-text stress, thread budget 1

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 339.4 | 5.4 |
| 7zz | 199.0 | 9.6 |
| bzip2 (Rust) | 147.7 | 3.5 |
| bzip2 (C) | 133.0 | 3.5 |

### Repeated-text stress, thread budget 2

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 336.6 | 5.2 |
| 7zz | 207.1 | 9.7 |
| bzip2 (Rust) | 148.0 | 3.5 |
| bzip2 (C) | 133.2 | 3.5 |

### H.264/AAC MKV, thread budget 1

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 22.8 | 5.4 |
| 7zz | 23.6 | 9.7 |
| bzip2 (Rust) | 17.2 | 3.5 |
| bzip2 (C) | 19.5 | 3.5 |

### H.264/AAC MKV, thread budget 2

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 41.6 | 5.5 |
| 7zz | 41.9 | 9.8 |
| bzip2 (Rust) | 17.2 | 3.5 |
| bzip2 (C) | 19.5 | 3.5 |

### Generated ELF, thread budget 1

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 36.1 | 5.4 |
| 7zz | 36.4 | 9.7 |
| bzip2 (Rust) | 24.5 | 3.5 |
| bzip2 (C) | 26.1 | 3.5 |

### Generated ELF, thread budget 2

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 54.1 | 5.5 |
| 7zz | 52.4 | 9.8 |
| bzip2 (Rust) | 24.6 | 3.5 |
| bzip2 (C) | 26.1 | 3.5 |

The canonical fixture identities and generation details appear with the earlier matrix below. Raw reports, frozen binary identities and a signed-source build receipt are retained outside Git. These results use uniform null-device output; earlier measurements used Rust in-memory sinks. Do not combine their medians or attribute the entire difference between batches to code changes.

See the [decoder implementation notes](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/decoder-performance.md) for retained improvements and verification. Intel decoder timings below belong to the earlier runtime; the latest decoder has not been rebenchmarked on Intel.

## Earlier four-way baseline

The following encoding and decoding matrix is preserved at its original source identity (`92a46108a726`, runtime `5a6c6cb7ded96`). Its decoder predates the improvements above. Rust codecs used their in-memory sink while 7zz wrote to the operating-system null device.

Measured 2026-11-05: four complete codec processes, four balanced measured rounds after one warmup. Primary tables use level 5, 500,000-byte blocks, one pass and one thread. Throughput uses original uncompressed input bytes; RSS is median peak codec-process RSS from GNU time's own child report (`gnu-time-child-v1`). Wall time includes GNU time wrapper startup, codec startup and I/O. Values are per workload, without an aggregate score.

`bzip2` 0.6.1 uses either the pure Rust `libbz2-rs-sys` 0.2.6 backend or bundled libbz2 1.0.8. Relative performance varies by workload and operation; gains against earlier bzip2-turbo versions do not imply a lead over every competitor. These measurements cover this five-fixture matrix; full ARM/x86 performance parity is not claimed.

Measured on Ubuntu 24.04 with 16 GiB RAM per host: c7g.2xlarge (Graviton3) and c7i.2xlarge (Intel Xeon Platinum 8488C). Rust 1.97.1 builds use O3, fat LTO, one codegen unit and stripping. 7zz 26.04 uses pinned 7-Zip source `9128b80e3a47` and its stock GCC O2 Makefile.

Input sizes and 7zz level-5 encoded/input byte fractions (lower means smaller output; the same encoded bytes were verified on both hosts):

| Fixture | Bytes | Output/input |
|---|---:|---:|
| JSON | 16,777,216 | 0.144587 |
| Repeated text | 16,777,216 | 0.000499 |
| Logs | 16,777,222 | 0.093477 |
| MKV | 16,360,253 | 1.000513 |
| ELF | 3,903,413 | 0.587674 |

### Graviton3: 8 physical cores, 8 vCPUs

#### Varied JSON

Input: 16,777,216 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 15.0 | 6.9 |
| 7zz | 10.6 | 11.5 |
| bzip2 (Rust) | 14.7 | 5.2 |
| bzip2 (C) | 13.8 | 5.3 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 38.0 | 4.5 |
| 7zz | 39.7 | 9.7 |
| bzip2 (Rust) | 30.3 | 3.5 |
| bzip2 (C) | 30.2 | 3.5 |

#### Repeated sentence stress

Input: 16,777,216 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 5.6 | 6.5 |
| 7zz | 3.0 | 11.2 |
| bzip2 (Rust) | 5.3 | 5.7 |
| bzip2 (C) | 4.6 | 5.6 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 340.0 | 4.5 |
| 7zz | 197.9 | 9.6 |
| bzip2 (Rust) | 148.8 | 3.5 |
| bzip2 (C) | 133.9 | 3.5 |

#### Structured logs

Input: 16,777,222 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 17.8 | 6.7 |
| 7zz | 11.7 | 11.4 |
| bzip2 (Rust) | 12.9 | 5.3 |
| bzip2 (C) | 11.5 | 5.2 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 57.6 | 4.6 |
| 7zz | 59.9 | 9.7 |
| bzip2 (Rust) | 49.4 | 3.5 |
| bzip2 (C) | 45.7 | 3.5 |

#### H.264/AAC Matroska

Input: 16,360,253 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 11.7 | 7.6 |
| 7zz | 9.2 | 12.2 |
| bzip2 (Rust) | 8.0 | 5.2 |
| bzip2 (C) | 9.6 | 5.3 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 19.3 | 4.6 |
| 7zz | 21.3 | 9.7 |
| bzip2 (Rust) | 16.9 | 3.5 |
| bzip2 (C) | 19.5 | 3.5 |

#### Linux executable

Input: 3,903,413 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 13.0 | 7.6 |
| 7zz | 10.2 | 12.2 |
| bzip2 (Rust) | 11.9 | 5.3 |
| bzip2 (C) | 12.7 | 5.2 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 32.1 | 4.5 |
| 7zz | 33.0 | 9.7 |
| bzip2 (Rust) | 23.5 | 3.5 |
| bzip2 (C) | 25.4 | 3.5 |

#### Eight-thread budget

Only turbo and 7-Zip accept this thread budget. The pure Rust and C backends remain serial; use their primary rows above. All four engines' separate measurements, including their RSS, remain in the raw reports.

Encode MiB/s:

| Fixture | bzip2-turbo | 7zz |
|---|---:|---:|
| JSON | 86.9 | 66.1 |
| Repeated text | 24.5 | 15.4 |
| Logs | 105.4 | 78.2 |
| MKV | 71.7 | 62.1 |
| ELF | 66.1 | 57.0 |

Decode MiB/s:

| Fixture | bzip2-turbo | 7zz |
|---|---:|---:|
| JSON | 43.6 | 47.2 |
| Repeated text | 336.3 | 206.4 |
| Logs | 65.3 | 69.0 |
| MKV | 37.8 | 41.6 |
| ELF | 46.8 | 49.6 |

Encode peak RSS MiB:

| Fixture | bzip2-turbo | 7zz |
|---|---:|---:|
| JSON | 41.1 | 58.8 |
| Repeated text | 39.1 | 56.8 |
| Logs | 40.2 | 57.8 |
| MKV | 47.9 | 63.9 |
| ELF | 43.8 | 59.4 |

Decode peak RSS MiB:

| Fixture | bzip2-turbo | 7zz |
|---|---:|---:|
| JSON | 5.7 | 9.8 |
| Repeated text | 5.2 | 9.7 |
| Logs | 5.1 | 9.8 |
| MKV | 6.1 | 9.8 |
| ELF | 5.7 | 9.8 |

### Intel Xeon: 4 physical cores, 8 SMT vCPUs

#### Varied JSON

Input: 16,777,216 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 17.1 | 7.4 |
| 7zz | 11.8 | 12.2 |
| bzip2 (Rust) | 16.1 | 5.7 |
| bzip2 (C) | 15.7 | 5.7 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 83.5 | 5.1 |
| 7zz | 86.4 | 10.3 |
| bzip2 (Rust) | 50.7 | 3.9 |
| bzip2 (C) | 47.6 | 3.9 |

#### Repeated sentence stress

Input: 16,777,216 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 8.4 | 7.0 |
| 7zz | 5.7 | 11.8 |
| bzip2 (Rust) | 6.4 | 6.2 |
| bzip2 (C) | 5.4 | 6.1 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 393.2 | 5.1 |
| 7zz | 273.4 | 10.3 |
| bzip2 (Rust) | 180.7 | 3.9 |
| bzip2 (C) | 167.4 | 4.0 |

#### Structured logs

Input: 16,777,222 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 20.2 | 7.2 |
| 7zz | 12.5 | 12.1 |
| bzip2 (Rust) | 14.8 | 5.7 |
| bzip2 (C) | 13.5 | 5.6 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 111.6 | 4.9 |
| 7zz | 117.7 | 10.4 |
| bzip2 (Rust) | 72.7 | 3.9 |
| bzip2 (C) | 65.9 | 3.9 |

#### H.264/AAC Matroska

Input: 16,360,253 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 14.1 | 8.1 |
| 7zz | 11.8 | 12.8 |
| bzip2 (Rust) | 12.1 | 5.7 |
| bzip2 (C) | 11.6 | 5.6 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 45.5 | 5.0 |
| 7zz | 33.4 | 10.4 |
| bzip2 (Rust) | 19.6 | 3.9 |
| bzip2 (C) | 21.2 | 3.9 |

#### Linux executable

Input: 3,903,413 bytes.

| Engine | Encode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 15.1 | 8.1 |
| 7zz | 12.0 | 12.9 |
| bzip2 (Rust) | 15.7 | 5.6 |
| bzip2 (C) | 15.1 | 5.6 |

| Engine | Decode MiB/s | RSS MiB |
|---|---:|---:|
| bzip2-turbo | 56.5 | 5.1 |
| 7zz | 46.8 | 10.4 |
| bzip2 (Rust) | 27.7 | 4.0 |
| bzip2 (C) | 27.6 | 3.9 |

#### Eight-thread budget

Only turbo and 7-Zip accept this thread budget. The pure Rust and C backends remain serial; use their primary rows above. All four engines' separate measurements, including their RSS, remain in the raw reports.

Encode MiB/s:

| Fixture | bzip2-turbo | 7zz |
|---|---:|---:|
| JSON | 72.2 | 59.7 |
| Repeated text | 25.0 | 17.8 |
| Logs | 83.1 | 68.6 |
| MKV | 58.3 | 52.7 |
| ELF | 56.8 | 49.3 |

Decode MiB/s:

| Fixture | bzip2-turbo | 7zz |
|---|---:|---:|
| JSON | 110.8 | 122.6 |
| Repeated text | 349.2 | 280.3 |
| Logs | 133.3 | 151.1 |
| MKV | 74.0 | 41.7 |
| ELF | 82.5 | 59.5 |

Encode peak RSS MiB:

| Fixture | bzip2-turbo | 7zz |
|---|---:|---:|
| JSON | 42.0 | 59.5 |
| Repeated text | 39.8 | 57.4 |
| Logs | 40.9 | 58.5 |
| MKV | 48.5 | 64.5 |
| ELF | 44.6 | 60.0 |

Decode peak RSS MiB:

| Fixture | bzip2-turbo | 7zz |
|---|---:|---:|
| JSON | 6.2 | 10.6 |
| Repeated text | 5.8 | 10.4 |
| Logs | 5.6 | 10.5 |
| MKV | 6.6 | 10.6 |
| ELF | 6.2 | 10.5 |

Benchmark source: `92a46108a726`; runtime implementation: `5a6c6cb7ded96`; source content SHA-256 prefix: `be946ddf0de3`. The raw matrix additionally covers levels 1 and 9 with one thread. Every encoder output was cross-decoded by all four engines before timing; native bzip2-turbo/7zz encoded bytes matched. All timed decoders read the same 7zz stream.

JSON contains recurring attributes with varied values. Logs contain complete varied structured records. Repeated text is a continuous 71-byte sentence and is labeled periodic stress. Media is a 150-second H.264/AAC excerpt of [Big Buck Bunny](https://peach.blender.org/about/), Copyright 2008 Blender Foundation, [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/), trimmed from 30 seconds and remuxed to Matroska with replaced attribution metadata. ELF is an actual reproducible Go-built Linux executable. Containers and executables use their actual byte counts.

Canonical media was generated with FFmpeg 8.1.1 and the ELF with Go 1.27.0, then the same fixture bytes were copied to both hosts.

Canonical input SHA-256 prefixes (full values remain in fixture receipts):

- Varied JSON: `f7858958c4a1`
- Repeated sentence stress: `8befb6b76223`
- Structured logs: `170a8ba5218e`
- H.264/AAC Matroska: `a88533696bc7`
- Linux executable: `5d993bf86e33`

See the [reproduction instructions](https://github.com/scryer-media/bzip2-turbo/blob/main/bench/bzip2-turbo-bench/README.md) for canonical fixture generation, receipts, isolated engine builds and benchmark commands. Generated inputs, raw reports and profiling artifacts are kept outside Git. Build receipts separately bind source archives and toolchains to frozen executable hashes; report consistency checks are not a substitute for that build proof.
## License

The translated codec is LGPL-2.1-or-later: [COPYING](https://github.com/scryer-media/bzip2-turbo/blob/main/COPYING) and
[7-Zip notices](https://github.com/scryer-media/bzip2-turbo/blob/main/LICENSE-7ZIP). The compatibility adapters and API definitions
retain [MIT notices](https://github.com/scryer-media/bzip2-turbo/blob/main/LICENSE-BZIP2-MIT). Package metadata declares both.
No libbz2 runtime or C codec build is required by the library. Standard bzip2
is a dev-only oracle; optional standalone benchmark tools can build either its
pure Rust or bundled C backend. See the [publishing prerequisites](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/publishing.md)
for the operator-owned first publication and downstream integration steps.
