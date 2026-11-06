# Security

## Reporting a vulnerability

Report suspected vulnerabilities privately through GitHub's security advisory
form for this repository:
<https://github.com/scryer-media/bzip2-turbo/security/advisories/new>.
Do not open a public issue or pull request for a suspected vulnerability.

Include the crate version, the enabled features, the target triple, the API
used, reproduction steps and a minimal synthetic input. Do not include
credentials or private data. Reports are acknowledged through the advisory,
and a fix is published as a new crate version with a changelog entry.

## Threat model

The library decodes and encodes BZip2 streams for callers who may receive
those streams from untrusted sources.

In scope:

- Malformed, truncated or adversarial compressed input must return an error
  without out-of-bounds access, uninitialized reads, unbounded allocation or
  panics on the decode path. Decoder storage is bounded by the validated
  block-size header, worker queues are bounded, and workers are joined on
  teardown.
- Memory safety of the unsafe code. The crate denies `unsafe_code` except in
  `tt_storage`, whose Linux x86-64 and AArch64 huge-page path maps, advises
  and unmaps anonymous memory through libc. Every other module, including the
  parsers, the inverse-BWT reconstruction and the compatibility adapters, is
  safe Rust.
- Checksum enforcement: each block's CRC is verified as it is reconstructed
  and the stream's combined CRC at its end; a mismatch is an error. Read a
  decoder through EOF to get the stream check.

Out of scope, left to the caller:

- Output size. A small compressed stream can decode to a very large output.
  The library streams output and does not cap it; callers decoding untrusted
  input must enforce their own output limit.
- Timing and other side channels. The codec is not constant-time and handles
  no secrets.
- Resource limits beyond the codec's own allocations, such as the thread
  budget passed in `EncoderOptions` and `Decoder` settings.

## Unsafe code: mitigations and compensating controls

All `unsafe` in the crate is the `native` module of `src/tt_storage.rs`: three
libc declarations (`mmap`, `munmap`, `madvise`), the calls to them, two slice
constructors over the mapping, and the `Send` and `Sync` impls for the buffer
that owns it. It is compiled only for Linux x86-64 and AArch64 and never
under Miri.

Design mitigations:

- The crate root carries `#![deny(unsafe_code)]`, lifted only by the single
  `#[allow(unsafe_code)]` on that module, and the `decoder` and `encoder`
  modules, which hold the parsers and reconstruction, carry
  `#[forbid(unsafe_code)]`. New unsafe code elsewhere fails to compile.
- Every unsafe block has a `SAFETY` comment stating the invariant it relies
  on. The mapping is anonymous, private, read/write and kernel-zeroed, so
  every byte is initialized before any read; the element types are sealed to
  `u8`, `u32` and `[u32; 2]`, which accept all-zero bits.
- Sizes are validated before mapping: zero, above the element type's
  ceiling (2, 4 or 8 MiB) or not a 2 MiB multiple is refused. The buffer's logical length is
  asserted against its capacity on every change, and slices expose only the
  logical length.
- The mapping is uniquely owned. `Drop` unmaps exactly the extent it kept;
  slack outside the aligned extent is unmapped at allocation time. Shared
  access hands out only immutable slices, mutation needs `&mut self`, and no
  raw pointer escapes the module.
- Any mapping failure falls back to an ordinary `Vec`, so the codec is
  correct, and exercised by the same tests, without the unsafe path.
- `madvise` advice is applied before first touch and its failure is ignored;
  it changes page size, never memory contents.

Verification controls, run on every pull request and release:

- The module's own tests check the slack arithmetic, 2 MiB alignment and
  exact capacity, that dropped extents and slack return to the kernel
  (measured through `/proc/self`), that failed mappings fall back to a `Vec`,
  that refused sizes and advice failures keep zeroed storage, that a panic
  in the advice callback still unmaps during unwind, and that shared words
  are read-only until every borrow ends.
- The test suite runs natively on Linux x86-64 and ARM64, macOS ARM64 and
  Intel, and Windows x86-64 and ARM64, so the mapped path and the portable
  path are both exercised with every feature set.
- Miri with strict provenance runs the `tt_storage` tests on the portable
  path. Miri cannot execute the OS calls, so it establishes the ownership and
  length logic around the mapping, not the libc ABI itself; the native test
  lanes above cover that.
- Two libFuzzer targets, `decode` and `roundtrip`, run under
  AddressSanitizer on every change, feeding malformed and generated streams
  through the decoder and its mapped storage.
- Clippy with warnings denied on all targets and both feature sets, CodeQL
  for Rust, `cargo audit` on the dependency graph, and zizmor on the
  workflows themselves.

The [validation record](docs/validation.md) lists the runs behind these
controls and which of them do not establish OS FFI or SIMD safety.
