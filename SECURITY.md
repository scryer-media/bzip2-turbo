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

The [validation record](docs/validation.md) states which checks have been run
and which of them do not establish OS FFI or SIMD safety.
