# Porting notes

The codec translates 7-Zip revision
`9128b80e3a471108678e2b3ec3c985861c8d7b0f` (26.04), preserving LGPL-2.1-or-later.
The extraction baseline is sevenz-turbo commit `e704d1e`. The reference files are
`CPP/7zip/Compress/BZip2Encoder.cpp`, `BZip2Decoder.cpp`, their headers, and the
public-domain block sorting and Huffman helpers under `C/`. Source comments
record translated routines. The Rust bzip2 0.6.1 adapters and API definitions
are reused under MIT; see `LICENSE-BZIP2-MIT`. This is an independent crate.

## Source and API mapping

| Reference or responsibility | Rust implementation |
| --- | --- |
| 7-Zip encoder stream and worker slots | `Encoder`, `encoder/slot.rs` |
| Block sorting, entropy coding and trials | `encoder/flag_sort.rs`, `entropy.rs`, `trial.rs` |
| 7-Zip decoder parsing and coordination | `Decoder`, `decoder.rs` |
| TT reconstruction | `decoder/compact.rs`, `paired.rs`, `tt.rs` |
| Bounded TT ownership and native allocation | `tt_storage.rs` |
| bzip2 0.6.1 public adapter interfaces | `read`, `write`, `bufread` |
| Resumable in-memory interface | `Compress`, `Decompress`, `raw.rs`, `decoder/incremental.rs` |
| CRC-32/BZIP2 | `crc_turbo::Digest`, shared in `lib.rs` |

The compatibility parser retains explicit bit-level state between calls. It
neither replays partial compressed blocks nor buffers the entire input stream.
The native `Decoder` and `Encoder` APIs retain the existing optimized paths and
explicit threading controls. The compatibility APIs use one thread.

## Behavioral contracts

- Compatibility `Compression` levels select 100,000-byte block increments;
  its default is six. Native `EncoderOptions` preserves 7-Zip level/pass mapping
  and its level-five default. Matching native options preserves 7-Zip bytes;
  libbz2 compressed-byte identity is not promised.
- `BzDecoder` stops at one member. `MultiBzDecoder` reads concatenated members.
  Native `Decoder` defaults to multistream. Use `bufread` to retain trailing
  bytes; `read` owns a `BufReader` and can read ahead.
- `Compress::new` validates `work_factor` in 0..=250, but the 7-Zip sorting
  algorithm does not use libbz2's fallback-work tuning. This is a documented
  tuning difference, not an equivalent implementation of that parameter.
- `Decompress::new(true)` selects compact 32-bit TT storage. It does not use
  libbz2's 16-bit small-memory representation or promise its memory footprint.
- Low-level calls can consume or produce less than their slice capacity.
  The compatibility bridge admits at most 16 KiB input per compression call;
  safe uninitialized-output helpers use at most 64 KiB scratch per call.
  Callers must use counters and returned status, as with the reference API.
- Compatibility writer `flush`, `try_finish`, `finish` and Drop follow the
  adapter contract. Native `Encoder::flush` deliberately preserves its current
  block, and native callers must explicitly call `finish`.
- A zero-progress underlying writer returns `WriteZero`; the copied adapter's
  endless zero-progress loop is deliberately corrected.
- The `compress` feature is enabled by default. Disabling defaults leaves
  decoding available. Backend/linkage features from the original bzip2 package
  are not applicable and must be removed when migrating Cargo feature lists.

## Validation mapping

Native encoder tests compare exact bytes with the pinned 7-Zip oracle across
levels, blocks, passes and thread counts. Decoder tests cover randomized
blocks, malformed tables, CRCs, output boundaries, multistream behavior and
worker teardown. `tests/raw_compat.rs` and `tests/stream_compat.rs` exercise the
compatibility API against bzip2 0.6.1, including tiny chunks, counters, short I/O,
trailing data and errors. Generated fixtures are not committed.

`tt_storage` native allocation/advice FFI is checked by native tests; portable
ownership has focused Miri coverage. Miri does not verify OS FFI or crc-turbo's
architecture kernels. See [validation](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/validation.md) for actual results
and [implementation history](https://github.com/scryer-media/bzip2-turbo/blob/main/docs/implementation-history.md) for preserved
pre-extraction evidence. Current performance is the first-pass baseline;
full cross-platform performance parity has not been established.
