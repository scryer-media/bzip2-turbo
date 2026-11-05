# BZip2 process benchmark harness

Standard-library-only Go harness with `toolchain`, `run`, and `suite` commands.
`legacy-corpus DESTINATION --mib 32` and `legacy-compare --codec EXE --oracle
EXE --input FILE --results JSON` preserve the historical two-process tools,
including byte-identical xorshift corpora and the original JSON report keys.
The latter accepts the historical level, block-size, passes, runs, warmups,
hardware, revision, profile, darwin-resources and require-parity options.
Platform identity values use Go OS/architecture names. These legacy commands
keep their candidate/reference ratio direction; four-contender reports use
reference/ours. All raw outputs stay outside the checkout.
Four contenders run in rotating order: 7zz, turbo, Rust bzip2 with its pure Rust
backend, and Rust bzip2 with its C backend. Every encoded output is decoded by
every contender before timing. Native turbo output must match 7zz byte for byte;
other encoders need decoded-byte parity, not compressed-byte identity.

Build the native example and both isolated reference builds as documented in
`../../tools/bzip2-reference/README.md`, then run from this directory:

```sh
cargo build --manifest-path ../../Cargo.toml --release --example codec-bench
go run . toolchain --sevenzip 7zz
go run . suite --sevenzip 7zz --size-kib 16 --levels 1,5,9 --threads 1,2 --api both --repeats 4 --out /tmp/bzip2-smoke.json
# Full matrix, only when explicitly measuring performance on an idle host:
go run . suite --sevenzip 7zz --size-kib 32768 --levels 1,5,9 --threads 1,2,4,8 --api both --repeats 4 --out /tmp/bzip2-matrix.json
go run . run --sevenzip 7zz --input /tmp/corpus.bin --threads 1 --out /tmp/bzip2-run.json
```

The turbo example and both reference builds share release fat LTO, one codegen
unit, optimization level 3 and stripping. Keep the Rust toolchain, profile
overrides and any target-CPU override identical across those three builds; the
documented commands use the default target CPU baseline without
`target-cpu=native`. Record C-backend and 7zz compiler settings separately.
See the reference-build README for explicit equivalent environment overrides.

`--driver`, `--pure`, and `--c` select alternate driver builds; `--repo` selects
the source identity root. No tools are built or downloaded automatically.
`--list` prints the matrix without requiring tools. Corpora are deterministic
text, random, periodic and runs. Compatibility turbo is serial and skipped for
thread counts above one. Pure Rust and C references always run one real thread;
`reference_threads` records this even when native turbo and 7zz use more threads.

Default `--settings block-size` gives the setting index an explicit meaning:
`--levels 1,5,9` selects 100,000, 500,000 and 900,000-byte blocks. Native turbo
and 7zz receive that block size and one optimization pass explicitly; standard
bzip2 uses its corresponding compression setting and single standard pipeline.
Every output header is checked against its declared effective block size.
Matching block/pass counts do not make the encoders' algorithms identical.

Optional `--settings sevenzip-level` retains historical native 7zip defaults.
For example native level 5 uses a 900,000-byte block while standard bzip2 level 5
uses 500,000; native level 9 uses seven recursive optimization passes while the
standard pipeline uses one. These rows explicitly report each contender's
`block_bytes`, `optimization_passes`, policy and whether block/pass counts match.
Compatibility turbo always uses standard block sizing and one pass, and 7zz is
aligned to it even under the historical setting policy. Such historical rows
must not be interpreted as equal-setting performance comparisons.

At least three measured rounds are required; four rounds balance all four
starting positions. Warmups are discarded. Each encode/decode scenario produces
three reference-versus-turbo rows from the same four-contender timing rounds.
Decode timings use the same 7zz stream for all contenders. Reports contain raw
wall/CPU samples, median wall and peak RSS, reference-over-turbo wall/RSS ratios,
encoded sizes and identities, binary identities, and a digest of codec, driver,
reference manifests/lockfiles and Go harness source. Values above one favor turbo.
The optional turbo compatibility API retains the turbo contender identity.

Wall time includes process startup and I/O. RSS measures one child, excluding
descendants; macOS bytes and Linux KiB are normalized to bytes. Unsupported
platforms omit RSS. Hardware counters and host idleness are not assessed.
Tiny inputs only verify the harness and correctness; they are not performance
evidence. Raw reports and corpus files must remain outside the repository,
including through symlink aliases. No optimization is performed by this harness.
