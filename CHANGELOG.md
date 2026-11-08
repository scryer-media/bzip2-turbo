# Changelog

## 0.2.0

- Decode with every thread of the budget. `Decoder::with_threads(reader, N)`
  with N above one now runs N block workers (budgets clamp to 1..=64;
  WebAssembly stays single-threaded) instead of at most one reconstruction
  worker. The calling thread reads input in 1 MiB chunks and scans every bit
  offset for the block magic; workers decode each candidate speculatively
  (entropy decode, inverse BWT, output), and an in-order chain keeps a
  candidate only where the previous block, stream trailer and any next stream
  header end, discarding magics that occur inside block data. Output order,
  concatenated streams, randomised blocks, block and stream CRCs and the
  error-not-panic behaviour are unchanged.
- Bound multithreaded decoder memory by the budget: N workers, at most 2N open
  blocks holding at most 2 MiB of undelivered output each, and input read no
  further than a fixed window past the oldest block still needed.
  `memory_usage` and `low_memory_usage` grow linearly with N and report this
  bound; multithreaded workers use the compact inverse-BWT table on every
  platform, so on macOS ARM64 `memory_usage(N)` equals `low_memory_usage(N)`
  for N above one. A block encoded beyond a worker's 1.25 MiB lookahead, which
  ordinary encoders never produce, is decoded serially from there on. At
  N = 16 on macOS ARM64 the bound is 305.5 MiB.
- With an output callback and several threads, workers call it one chunk at a
  time in stream order. Dropping or `into_inner` cancels and joins every
  worker.
- Encode the two halves of a recursive optimisation trial (two passes by
  default at levels 7 to 9, or any explicit `passes` above one) on a spare
  thread of the encoder's budget while the encoder drains its last blocks
  (`finish`, `flush`, `flush_block`) and fewer blocks than threads are in
  flight. Output bytes are unchanged.
- Sort each block's rotations with a cyclic induced sorter (SA-IS class,
  written for rotations: no sentinel and no doubled input). A block is
  primitive when it is not a repetition of a shorter string; a primitive
  block has exactly one rotation order, so the BWT and origin, and therefore
  the output bytes, equal the previous sorter's. Blocks below 2,048 bytes and
  repetitions keep the previous sorter. The two sorters share one buffer, so
  sort memory is the larger of their needs: at a 900,000-byte block about
  7.5 MB on macOS ARM64 and 8.1 MB elsewhere.
- Level 9 now runs two optimisation passes by default instead of seven.
  `EncoderOptions { passes: Some(7), .. }` keeps the previous level-9 output,
  which is 7-Zip's level 9 byte for byte; the new default matches 7-Zip's
  `-mx=9 -mpass=2`. Measured on an Apple M5 Max: a 256 MiB JSON corpus
  encodes to the same 37,919,682 bytes either way; a 96 MiB corpus of build
  binaries grows from 39,501,755 to 40,118,359 bytes (+1.6%). Both stay below
  `lbzip2 -9` (38,092,338 and 40,423,634 bytes).
- Encoder scratch (sort, entropy and trial buffers) now belongs to threads:
  each worker, the calling thread and each lent helper owns one. A block slot
  keeps only its input and its encoded output, and up to four slots per
  thread may be open, so workers keep running while an earlier block
  finishes. Output bytes are unchanged.

## 0.1.1

- Decode native parser Huffman symbols with 7-Zip's `CDecoder` structure: one
  20-bit window, a 9-bit direct table for short codes and per-length code limits
  for longer codes, with no bit-serial reads. Incomplete and oversubscribed
  trees keep their existing acceptance and rejection, and decoded bytes are
  unchanged.
- Scatter inverse-BWT links through a per-block table of bucket counter
  references, two positions per step, so each counter is addressed through one
  register as in 7-Zip's `DecodeBlock1`; x86-64 level 5 and 9 text decoding
  comes within 2 percent of 7zz on the measured Intel host.
- Lay out the Linux AArch64 MTF carry loop as GCC lays out 7-Zip's
  `ReadBlock2` loop, two words per iteration with an exit test after each;
  film streams decode 1 to 3 percent faster on Graviton3, bytes unchanged.
- On Linux AArch64, refill the bit reservoir as `ReadBlock2` does, up to
  three bytes each behind its own test, on blocks whose Huffman tables average
  at least 6 expected bits per code, instead of one word when it runs low.
  The word refill's branch mispredicted on such high-entropy blocks there;
  film now decodes within 1 to 2 percent of 7zz at one thread on Graviton3
  and ahead at two, and the text cases, which keep the word refill, 1 to 4
  percent ahead. x86-64 keeps the word refill, which measured faster on it.
- Remove per-byte bounds-check branches from the inverse-BWT link scatter and
  paired successor pass with safe, branch-free index clamping.
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
- Add contributor rules for version-bump PRs: every code change bumps the
  version, `Cargo.lock` and changelog in the same PR, sized by
  `cargo semver-checks`, under a released `## <version>` heading.
- Present the README benchmarks as separate encode and decode tables per
  platform, narrow enough for crates.io, with MiB/s in the header, and show
  the serial `bzip2` references at every thread budget instead of only at 1T.
- Build tests at `opt-level = 1`: the native test suite runs in about 7 s
  instead of 60 s, with debug assertions and overflow checks still on.

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
- Add reproducible JSON, logs, repeated-text, media and executable fixtures, plus
  corrected codec-process RSS accounting and verified four-way benchmarks.
- Preserve measured baselines and remaining gaps without claiming complete parity.
