# Isolated standard bzip2 reference drivers

From the repository root:

```sh
cargo build --locked --release --manifest-path tools/bzip2-reference/Cargo.toml --features pure --target-dir target/reference-pure
cargo build --locked --release --manifest-path tools/bzip2-reference/Cargo.toml --features c --target-dir target/reference-c
cargo tree --locked --manifest-path tools/bzip2-reference/Cargo.toml --features pure
cargo tree --locked --manifest-path tools/bzip2-reference/Cargo.toml --features c
```

Both this workspace and the turbo root workspace use the same release profile:
`lto = "fat"`, `codegen-units = 1`, `opt-level = 3`, and `strip = true`.
The commands above therefore match the Rust release settings used by
`cargo build --locked --release --example codec-bench` from the repository root.
Use the same Rust toolchain and profile environment overrides for all three
Rust executables. For an explicit equivalent override, set
`CARGO_PROFILE_RELEASE_LTO=fat`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`,
`CARGO_PROFILE_RELEASE_OPT_LEVEL=3`, and `CARGO_PROFILE_RELEASE_STRIP=true`
for every build, including the turbo example.

These commands do not request `target-cpu=native`; Rust uses its default target
CPU baseline. If selecting a different CPU target, apply the same `RUSTFLAGS`
target-CPU override to all three Rust builds, record it with the measurements,
and rebuild all binaries. The bundled C backend uses its C build toolchain;
record any `CC`/`CFLAGS` overrides separately rather than treating Rust flags as
C compiler settings. Record the independently built 7zz toolchain and flags too.

The binaries are `target/reference-pure/release/bzip2-reference-bench` and
`target/reference-c/release/bzip2-reference-bench`. Use `--version` for backend
identity. Both accept `encode|decode LEVEL THREADS INPUT OUTPUT|-`; threads must
be one and `-` output discards bytes. Optional `--api compat` selects the only
reference API. Nondefault block/pass options are rejected. Compression levels
select 100,000-byte block increments. The four-way harness's default
`--settings block-size` aligns turbo and 7zz to those increments and one pass;
its historical `sevenzip-level` policy records any effective-setting mismatch.

Features `pure` and `c` are mutually exclusive; neither is enabled by default.
The lockfile pins standard bzip2 0.6.1, pure `libbz2-rs-sys` 0.2.6 and C
`bzip2-sys` 0.1.13+1.0.8. The pure dependency graph excludes the C backend; the C
graph excludes the Rust backend and enables bundled static compilation. This
independent, nonpublishable workspace does not affect the library runtime graph.
