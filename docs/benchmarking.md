# Benchmarking

Use `bench/bzip2-turbo-bench`, the same Go harness pattern as the other codec
repositories. Build the Rust example in release mode and use the pinned 7-Zip
reference built with `go run .github/scripts/tool.go oracle`. The four contenders are
7zz, turbo, standard Rust bzip2 with its pure Rust backend, and standard Rust
bzip2 with its bundled static C backend. See the harness README
for commands. Keep generated inputs, JSON and profiles outside the checkout.

The main `text` input is a valid randomized JSON payload: recurring record
attributes contain varied seeded text, numbers, booleans, arrays and nested
objects. Generation streams bounded records and produces the exact requested
byte count; final padding is shorter than one record. The fixed xorshift64*
seed makes output reproducible without repeating a global text block.
From the harness directory, `go run . json-corpus --output /tmp/text.json
--bytes 33554432` retains a fixture outside the checkout. Inputs must contain
at least 256 bytes. Earlier repeated-sentence measurements remain historical
periodic stress results and do not represent this JSON text workload.

Build the two reference drivers independently; never enable both features:

```sh
cargo build --locked --release --manifest-path tools/bzip2-reference/Cargo.toml --features pure --target-dir target/reference-pure
cargo build --locked --release --manifest-path tools/bzip2-reference/Cargo.toml --features c --target-dir target/reference-c
cargo tree --locked --manifest-path tools/bzip2-reference/Cargo.toml --features pure
cargo tree --locked --manifest-path tools/bzip2-reference/Cargo.toml --features c
```

The standalone workspace is excluded from the published crate. Its exact
`bzip2` 0.6.1 dependency and lockfile resolve `libbz2-rs-sys` 0.2.6 for `pure`,
and `bzip2-sys` 0.1.13+1.0.8 for `c`. The pure graph must exclude `bzip2-sys`;
the C graph must exclude `libbz2-rs-sys`. Both drivers require one thread and
use the same standard I/O buffering as the turbo compatibility driver. Native
turbo and 7zz can use matching explicit worker counts; reports retain the
serial reference thread count. This is benchmark-only isolation, not a library
backend feature or runtime dependency.

The harness defaults to `--settings block-size`: indices 1, 5 and 9 mean
100,000, 500,000 and 900,000-byte blocks for every contender. Native turbo and
7zz receive explicit block sizes and one optimization pass. Standard bzip2
uses the corresponding compression setting and its standard pipeline. Every
output header is checked against the declared block size. Equal block/pass
counts do not make the encoder algorithms identical.

Optional `--settings sevenzip-level` preserves historical native defaults;
numeric levels alone then differ in effective settings. Native level 5 uses a
900,000-byte block versus standard bzip2's 500,000, and native level 9 uses
seven recursive passes versus the standard pipeline's one. Reports record
per-contender block bytes, passes, policy and matching flags. Interpret these
historical rows with those differences; they are not equal-setting comparisons.
Compatibility turbo always uses standard block sizing and one pass, with 7zz
aligned to it under either policy.

Compare explicit block, pass and thread settings. Verify
encoded bytes and cross-decoded output before timing. Run at least three
alternating rounds and report medians; use reference/ours ratios, where greater
than one means faster or smaller. Record source revision, binary hash, compiler,
machine, input hash, thread count, elapsed time and RSS with platform-correct
units. Instrumentation must be reported separately from uninstrumented timing.
Use CPU sampling, instructions retired where supported, and assembly to diagnose
costs before changing algorithms. Do not infer CRC throughput from fused decoder
samples.

The first-pass baseline is frozen by operator instruction. No further parity
optimization is required for this preparation. Full cross-platform 0% regression
parity remains an unachieved future objective; selected passing smoke rows do not
establish it. Preserve every validated improvement and unsuccessful experiment's
evidence. The Go harness retains `legacy-corpus` and `legacy-compare` commands
for the pre-extraction two-process protocol. Its repeated-sentence text corpus
is historical periodic stress, retained byte for byte rather than used as the
main text workload. Run from
`bench/bzip2-turbo-bench`: `go run . legacy-corpus DESTINATION --mib 32`
and `go run . legacy-compare --codec EXE --oracle EXE --input FILE --results JSON`.
The corpus bytes, CLI options, report keys, alternating order, Darwin resource
counters and strict unrounded parity criterion match the historical tools.
Platform identity fields use Go OS/architecture names; supply `--hardware` for
a precise machine description. Four 1 MiB corpus SHA-256 characterizations and
a tiny comparison schema/identity smoke check verified the migration.

## ARM CRC correction

Seven alternating diagnostic rounds against the actual 7-Zip CRC source found
crc-turbo 1.11.0 much faster for bulk updates: 64 KiB took approximately 1.23 us
versus 116.52 us, and 4 KiB took 80.59 ns versus 7,296.62 ns. The scalar reference
won tiny calls: one byte took 3.27 ns versus 11.34 ns. Both algorithms and carried
states were checked; assembly showed scalar tables versus PMULL/EOR3 folding.
Machine idleness was not established, so these are diagnostic figures. The
reference fuses CRC into reconstruction; the earlier whole-decoder profile did
not prove that its standalone CRC was faster. Raw artifacts and identities are
preserved outside Git in the task's CRC audit bundle.
