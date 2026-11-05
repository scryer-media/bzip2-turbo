# Validation

## Native decoder follow-up

Runtime commit `8a2188e6b667` passed the macOS ARM64 integrated checks with
Rust 1.97.1 and cargo-nextest 0.9.143. The all-feature sweep passed 123 tests
with zero skips; the decoding-only sweep passed 49 with zero skips. The pinned
7-Zip 26.04 oracle was enabled. Nextest reported one process-cleanup warning
on the table-rebuild test; its isolated rerun passed without the warning. The
broad run is therefore not described as cleanup-warning-free.

Formatting, strict all-target Clippy in both feature configurations, doctests
and strict rustdoc passed. A local `cargo package --locked` verified the crate
without publishing it. Both feature configurations compiled for Wasm using the
installed Rust 1.97.1 rustup target; the same-version Homebrew compiler lacked
that target. Privacy and redacted gitleaks checks passed before the SSH-signed
commit. No new unsafe code or dependency change was introduced.

The subsequent documentation pass changed comments and examples without
changing runtime behavior. Strict rustdoc with both `-D warnings` and
`-D missing_docs` passed in both feature configurations; the final examples
passed five all-feature and two decoding-only doctests.

Graviton native candidate checks and exact-output benchmarks supplement these
local tests; they do not substitute for the full platform CI matrix. See the
[decoder notes](decoder-performance.md) for retained changes and rejected
experiments. The final benchmark method and runtime identity are recorded in
the README. The following extraction record is historical, not a claim that
all of those checks were rerun on this commit.

## Extraction baseline

The standalone crate was checked locally on macOS. Completed native sweeps used
cargo-nextest 0.9.143 with `--no-fail-fast`: 116 all-feature tests passed with no
skips, and 43 decoding-only tests passed with no skips. The local sevenz-turbo
integration sweep passed 613 tests with eight expected ignored tests, using an
invocation-only Cargo patch to the standalone checkout. That patch is not part
of the downstream manifest: sevenz-turbo declares registry dependency
`bzip2-turbo = "0.1.0"`, so an operator-authorized crates.io publication is still
required for its unpatched packaging and CI. A targeted sevenz-turbo archive
sweep also passed all six BZip2 tests, including the normally ignored oracle
case. Its doctests passed three examples with one ignored.

Strict rustdoc passed with `RUSTDOCFLAGS=-Dwarnings cargo doc --locked
--all-features --no-deps`. Default-feature doctests passed both examples;
`cargo test --locked --no-default-features --doc` passed with zero doctests
(the crate-level encoding examples are feature-gated).

Final strict all-target Clippy passed for both default/all-feature and
decoding-only configurations, and for the fuzz workspace. The isolated
pure Rust and bundled C reference drivers each passed Clippy, a roundtrip,
compressed-byte comparison with each other, and five invalid-argument checks.
Their resolved dependency graphs contain only the selected codec backend.
Standalone `cargo package` verified a 45-file package that excludes benchmark,
fuzz, tool and CI content. Pinned Miri passed four focused tests with strict
provenance; this verifies portable ownership paths, not native OS FFI or
architecture CRC kernels. The downstream workspace passed strict all-target,
all-feature Clippy. The standalone default and decoding-only configurations
compiled for Wasm with the pinned 1.97.1 compiler.

The new decode and roundtrip fuzz targets each completed 100 AddressSanitizer
runs. Final candidate-tree privacy and redacted gitleaks checks passed for all
84 tracked and nonignored candidate files. Formatting checks passed for the
crate, fuzz workspace and both isolated tools. The large-stream harness passed
its standalone locked compile check. Actionlint passed, and regular-persona
zizmor reported no findings; pedantic mode reported 19 informational findings
(unnamed jobs and toolchain setup actions), with zero low, medium or high
findings. Fixed matrix selections pass through quoted environment variables.

Large-stream validation passed for text and valid RF64 media inputs of
4,295,032,832 bytes each. The media compressed stream was 4,329,555,102 bytes,
so both input and output exceeded the 32-bit boundary. Checks covered raw
single-threaded encode/decode counters, native single-threaded text and
multithreaded media callbacks, byte comparisons and independent 7zz decoding.
See [large-stream validation](large-streams.md) for identities and commands.
Remote platform CI has not yet been verified.
CI configuration alone does not establish that those jobs ran.

The pre-extraction signed baseline is sevenz-turbo `e704d1e`. Its historical
verification and performance results are preserved in
[implementation history](implementation-history.md); they do not certify the
new compatibility code. The first-pass performance baseline remains frozen;
no full ARM/x86 single-threaded or multithreaded parity claim is made.
