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
- Reduce native decoder parsing and output-storage work, reuse bounded parser
  scratch, and retain source-faithful Linux ARM64 MTF traversal.
- Reshape the compact inverse-BWT reconstruction loop after
  `CSpecState::Decode`, with minimal live state, a branch-free run counter and
  one limit compare per byte; decoded bytes and errors are unchanged.
- Remove per-byte bounds-check branches from the inverse-BWT link scatter and
  paired successor pass with safe, branch-free index clamping.
- Add reproducible JSON, logs, repeated-text, media and executable fixtures, plus
  corrected codec-process RSS accounting and verified four-way benchmarks.
- Preserve measured baselines and remaining gaps without claiming complete parity.
- Queue each parsed block for the decoder's reconstruction worker while it
  still reconstructs the previous block, matching 7-Zip's parser run-ahead.
- Back every decoder TT and last column on Linux x86-64/AArch64 with whole
  2 MiB pages advised for transparent huge pages, as 7-Zip's `BigAlloc` does,
  size the TT once at the declared block limit and recycle at most two last
  columns; the Linux single-threaded compact estimate is now 8 MiB.
