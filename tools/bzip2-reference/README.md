# Isolated standard bzip2 reference drivers

From the repository root:

```sh
cargo build --locked --release --manifest-path tools/bzip2-reference/Cargo.toml --features pure --target-dir target/reference-pure
cargo build --locked --release --manifest-path tools/bzip2-reference/Cargo.toml --features c --target-dir target/reference-c
cargo tree --locked --manifest-path tools/bzip2-reference/Cargo.toml --features pure
cargo tree --locked --manifest-path tools/bzip2-reference/Cargo.toml --features c
```

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
