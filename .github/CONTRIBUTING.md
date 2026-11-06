# Contributing

Read AGENTS.md before editing. Use a GitFlow work branch and SSH-signed
commits, enable the repository hooks, and keep private paths and credentials
out of all tracked files and messages. Required tools are Rust from
rust-toolchain.toml, Go, gitleaks and cargo-nextest 0.9.143.

Run formatting, Clippy, native tests with --no-fail-fast and doctests as
specified in AGENTS.md. Native conformance tests require 7-Zip; set
BZIP2_TURBO_7ZZ to the reference executable. Report the source revision or
installed reference version with validation results. Miri's portable storage
checks do not validate native allocation or advice FFI.

Performance changes need byte parity and interleaved median measurements.
Dependency changes need explicit approval. Never commit generated fixtures, fuzz inputs or raw benchmark data.
Release and publication remain separate operator actions.
