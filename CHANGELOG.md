# Changelog

## 0.1.0

- Extract the 7-Zip BZip2 port into an independent library with retained LGPL
  provenance and crc-turbo checksums.
- Preserve bounded native single-threaded and multithreaded encoding/decoding,
  compact memory selection, callbacks and explicit 7-Zip settings.
- Add bzip2 0.6.1-compatible streaming and low-level APIs with documented backend
  tuning and memory differences; retain MIT adapter notices.
- Add independent conformance, compatibility, fuzz, privacy, secret-scanning,
  platform CI and benchmark tooling.
- Improve encoder table pricing and block-sort partition scans while preserving
  reference output bytes.
- Decode native parser Huffman symbols with 7-Zip's `CDecoder` structure: one
  20-bit window, a 9-bit direct table for short codes and per-length code limits
  for longer codes, with no bit-serial reads. Incomplete and oversubscribed
  trees keep their existing acceptance and rejection, and decoded bytes are
  unchanged.
- Scatter inverse-BWT links through a per-block table of bucket counter
  references, two positions per step, so each counter is addressed through one
  register as in 7-Zip's `DecodeBlock1`; x86-64 level 5 and 9 decoding reaches
  7zz parity on the measured Intel host.
- Lay out the Linux AArch64 MTF carry loop as GCC lays out 7-Zip's
  `ReadBlock2` loop, two words per iteration with an exit test after each;
  film streams decode 1 to 3 percent faster on Graviton3, bytes unchanged.
- Reduce native decoder parsing and output-storage work, reuse bounded parser
  scratch, and retain source-faithful Linux ARM64 MTF traversal.
- Remove per-byte bounds-check branches from the inverse-BWT link scatter and
  paired successor pass with safe, branch-free index clamping.
- Add reproducible JSON, logs, repeated-text, media and executable fixtures, plus
  corrected codec-process RSS accounting and verified four-way benchmarks.
- Preserve measured baselines and remaining gaps without claiming complete parity.
- Queue each parsed block for the decoder's reconstruction worker while it
  still reconstructs the previous block, matching 7-Zip's parser run-ahead.
- Back every streaming `Decoder` TT and last column (the compatibility API's
  TT only) on Linux x86-64/AArch64 with whole 2 MiB pages advised for
  transparent huge pages, as 7-Zip's `BigAlloc` does, mapped with anonymous
  `mmap`/`munmap` independently of the process allocator and faulted in lazily
  without a pre-fill, size the TT once at the declared block limit and recycle
  at most two last columns; the Linux single-threaded compact estimate is now
  8 MiB. `Vec`-backed TT and column storage zero-fills only growth past the
  largest length it has held.
- Benchmark harness: decode is measured only at 1 and 2 threads, matching the
  native decoder's two-thread cap; higher budgets still measure encoding and are
  recorded as skipped decode scenarios. `legacy-compare` applies the same
  cap to its decode timings while still checking bytes at every thread count.
