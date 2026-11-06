# Implementation history before standalone extraction

This preserved record describes the internal sevenz-turbo implementation at
commit `e704d1e`. Paths and verification counts below refer to that historical
checkout, not the standalone compatibility API. Later results are recorded in
[validation](validation.md). Raw measurements remain outside the repository.

# BZip2 codec provenance and validation

This module translates the BZip2 codec in 7-Zip revision
`9128b80e3a471108678e2b3ec3c985861c8d7b0f` and retains LGPL-2.1-or-later
licensing. The encoder and decoder source notices identify translated routines.
The corresponding upstream files are `CPP/7zip/Compress/BZip2Encoder.cpp`,
`CPP/7zip/Compress/BZip2Decoder.cpp`, their headers, and the public-domain
block sorting and Huffman helpers under `C/`. The command-line oracle is
7-Zip built at that exact revision (26.04). Installed 26.01 was used only for
initial command-line characterization. No reference executable or generated
fixture bytes are bundled.

## Source layout and packaging

`src/codec/bzip2_turbo/mod.rs` contains the shared CRC support and exports;
`decoder.rs` contains parsing and worker coordination; concrete reconstruction
kernels live in `decoder/compact.rs` and `decoder/paired.rs`, with shared TT
construction in `decoder/tt.rs`. `encoder.rs` holds options and shared helpers;
`encoder/slot.rs` owns the streaming writer and reusable workers,
`encoder/flag_sort.rs` sorts blocks, `encoder/entropy.rs` fits and writes
Huffman tables, and `encoder/trial.rs` holds recursive trial output.
The earlier encoder and sorters remain test oracles. These files are packaged
with the parent crate under its `bzip2` feature. Encoding additionally requires
`compress`. The private, unpublished `crates/bzip2-turbo` workspace crate points
its library target at the same `mod.rs`, so its standalone conformance tests
exercise exactly the implementation used by the archive adapters. The private
crate enables `compress` by default and depends on published `crc-turbo`
1.11.0 with default features disabled and `std` enabled. The parent uses the
same dependency configuration, optional and enabled by its `bzip2` feature.
Standalone tests and benchmark scripts live under `crates/bzip2-turbo`.

BZip2's wire checksum is CRC-32/BZIP2: the non-reflected `0x04C11DB7`
polynomial with initial state and final XOR both `0xFFFFFFFF`. The codec
uses `crc_turbo::Digest` with `CrcAlgorithm::Crc32Bzip2`, carrying the raw
state between updates and applying the final complement at the format
boundary. Decoder output is checked in batches. This is separate from the
7z container's reflected CRC-32, which remains `crc-fast` through
`lzma-turbo`, including per-file folding. The `crc-host` archive feature does
not delegate BZip2 wire checksums to the host.

The encoder and decoder algorithms forbid unsafe code. The private
`tt_storage::native` module confines Linux allocation/advice FFI: it owns
aligned storage, initializes its complete allocation, exposes bounded slices
and frees it with the matching C allocator. Other platforms and Miri use Vec
storage. Native ownership tests and focused CI coverage are inventoried in
`docs/unsafe-validation.md`; Miri cannot verify the OS calls. Architecture-specific
SIMD or unsafe kernels selected by `crc-turbo` belong to that dependency and
are outside the algorithm modules' safe-Rust boundary. Codec differential tests verify
observable checksum results; they do not independently establish the safety
of a dependency's architecture kernels.

## API mapping

| Reference behavior | Rust API |
| --- | --- |
| BZip2 encoder input/output streams | `Encoder<W: Write>`, `Write`, `finish()` |
| Compression level and worker count | `EncoderOptions { level, threads, ..Default::default() }` |
| Dictionary-size and optimization-pass properties | `EncoderOptions::block_size`, `EncoderOptions::passes` |
| BZip2 decoder input/output streams | `Decoder<R: Read>`, `Read` |
| Decoder worker count | `Decoder::with_threads(reader, threads)` |
| Decode all streams / stop after the first stream | `Decoder::set_multistream(bool)` |
| Producer-side archive checksum integration | `Decoder::with_output_callback` |
| Conservative codec allocation estimate | `Decoder::memory_usage(threads)` |
| Explicit compact links and their allocation estimate | `Decoder::with_low_memory`, `with_low_memory_output_callback`, `low_memory_usage` |
| Archive compression options | `Bzip2Options::from_level`, `from_level_mt`, `set_block_size`, `set_passes` |

Thread counts default to one. Output block order must remain identical across
thread counts. `finish()` writes the stream trailer and returns the underlying
writer; callers must use it to complete a stream.

An encoder requesting N threads uses N−1 persistent workers and the caller.
There are at most N move-owned slots, including the slot currently being
filled. Each retains input, sort scratch, packed MTF bytes, fixed Huffman
scratch and recursive trial output. Jobs and results transfer the same owner;
ordered delivery returns it for reuse. The caller helps encode when acquiring
another slot or draining the stream. Errors and drop cancel queued work and
join active workers before releasing their storage.

The sorter's external-flags mode comes from the pinned reference. After all
hash reads finish, its 65,536-word hash region becomes a bitmap of at most
28,125 words and disjoint small-sort scratch of at most 4,096 words. Word scans
skip finished ranges without changing refinement order. Rotated-rank lookup
remains unchanged. Recursive output has a checked 3,897,740-byte ceiling;
4 KiB initialization chunks may extend up to 4,095 bytes beyond demand,
clamped to that ceiling. These are storage bounds, not peak RSS estimates.

## Characterized reference behavior

The default block-size multiplier is 1, 3, 5, 7, 9 for levels 1, 2, 3, 4, 5
respectively, and remains 9 through level 9. The block budget applies to the
first run-length representation, rather than raw input bytes. Levels 1–6 use
one optimization pass, levels 7–8 use two, and level 9 uses seven. Higher levels
can choose recursive block subdivisions. Four Huffman tuning iterations and
maximum encoder code length 16 are distinct from these optimization passes.
Equal-cost selector candidates choose the lowest table index. These choices
affect byte parity even when alternative encodings decode identically.

`BlockSort` / `SortGroup` retain the reference partition, refinement and tie
order. Rank storage rotates by the current prefix width: physical slot `p`
holds logical rank `(p + width) mod block_length`. Original-position writes
translate into this view, keeping live updates visible to later groups in the
same pass. This removes cyclic-address arithmetic from hot rank reads without
changing encoded bytes or the BWT origin index. Differential checks include
all-equal data, repeated periods, non-power-of-two lengths, capacity boundaries
and full blocks; a unit test checks coordinate changes, live writes and padding.

## Conformance mapping

No upstream test suite was imported. Runtime-generated characterization cases
cover empty input, singleton input, periodic text, all byte values, repeated
runs at the RLE boundaries, deterministic pseudorandom input, and multiple
blocks at the small block setting.

| Contract | Test |
| --- | --- |
| Exact reference encoding, levels 1–9 and workers 1/2/4 | `seven_zip_encoding_is_byte_exact_at_every_level` |
| Reference stream decoding, short reads and workers 1/2/4/8 | `seven_zip_streams_decode_with_short_reads_and_parallel_workers` |
| Explicit block sizes and pass counts | `explicit_block_sizes_and_optimization_passes_are_byte_exact` |
| All prefixes, corrupt CRC/origin, invalid group/selector counts | `malformed_headers_and_truncated_streams_return_errors` |
| Historical randomized block with a real byte toggle | `legacy_randomized_blocks_match_the_reference` |
| Valid incomplete/dummy trees and invalid oversubscribed trees | `incomplete_huffman_maps_are_valid_but_oversubscribed_maps_are_not` |
| Concatenated streams and partial/trailing next headers | `concatenation_and_partial_next_headers_preserve_completed_stream_output` |
| Large unary/two-symbol/periodic blocks and incremental flush | `encoder_matches_reference_on_periodic_and_run_block_boundaries` |
| Archive byte stability, thread counts, finite memory | `bzip2_archive_threads_preserve_bytes_and_memory_limited_reads` |
| Archive reference extraction | `seven_zip_accepts_the_internal_bzip2_archive_encoder` |
| Corrupt archive checksums at ST/MT | `damaged_bzip2_blocks_fail_in_both_archive_decode_modes` |
| Combined CRC, truncated trailer and final marker errors | `bzip2_trailers_are_validated_after_the_declared_file_bytes` |
| Output exceeding declared archive size | `bzip2_rejects_decoded_bytes_beyond_the_declared_folder_length` |
| AES padding and multiple file checksum boundaries | `encrypted_bzip2_workers_verify_multiple_file_boundaries` |
| CRC backend polynomial, alignment and tail parity | `crc_backend_matches_bitwise_polynomial_at_every_tail` |
| Raw CRC state carry across large kernel boundaries | `crc_backend_preserves_carried_states_at_large_kernel_boundaries` |
| Adaptive TT scatter versus scalar, including dirty reuse | `every_tt_word_matches_checked_for_runs_periods_random_and_edges` |
| Sampler size and eight/nine-miss selection boundaries | `sampler_size_cutoff_and_eight_miss_threshold_are_exact` |
| Bounded word scans at every alignment and tail | `scanned_runs_stop_at_every_word_and_tail_boundary` |
| Carried output runs, zero-sized buffers, randomized count bytes and CRC failure | `cold_pending_runs_preserve_every_output_edge_and_randomized_counts` |
| Literal word scans versus four-byte windows, every alignment and tail | `literal_word_scan_matches_quad_windows_at_every_alignment_and_edge` |
| Persistent RLE admission state across input fragments | `every_pending_run_and_fragment_preserves_checked_literal_byte_state` |
| Count-byte flush admitting one literal beyond the nominal block limit | `long_run_count_flush_retains_the_reference_limit_plus_one_literal` |
| One-to-three repeated literals crossing block boundaries | `literal_repeat_cuts_at_block_limit_preserve_all_streaming_state` |
| Smaller constant-array ranks versus full-size rotation order and origin | `finer_rank_dispatch_matches_the_full_size_array_at_each_capacity_edge` |
| Maximum archive block under the existing codec-plus-input budget | `maximum_bzip2_blocks_preserve_existing_archive_memory_admission` |
| Bounded output ownership and reuse | `output_owners_are_bounded_and_reused_without_reallocation` |
| Output and return-queue teardown | `worker_drop_disconnects_a_blocked_output_send_before_join`, `worker_drop_disconnects_a_blocked_owner_receive_before_join` |

The oracle is required: set `BZIP2_TURBO_7ZZ` to the exact-reference executable if `7zz`
is absent from PATH. Oracle startup or execution failures fail the tests.

## Deliberate interface differences and preserved quirks

The Rust interfaces use `Read`/`Write` and `io::Error` instead of COM streams
and HRESULTs. They do not expose affinity, thread groups, progress callbacks,
or the reference decoder's resumable COM state. The archive adapter retains
its existing default compression level six; the standalone encoder matches
the reference codec's default level five. Explicit equivalent options produce
the same encoded bytes.

For finite archive memory budgets, the archive adapter selects compact links
and single-threaded decoding even when a larger worker count is requested. This
keeps the codec within the archive's existing 8 MiB admission estimate. The
standalone decoder defaults to paired links on macOS ARM64 (10 MiB ST / 16 MiB
MT estimates), and compact links elsewhere (6 MiB ST / 12 MiB MT; the Linux
x86-64/AArch64 ST estimate later became 8 MiB with 2 MiB huge-page TT and
last-column storage, see [decoder performance](decoder-performance.md)). Explicit
low-memory constructors select compact links on every platform. These estimates
exclude caller-owned readers, callback allocations and OS thread stacks. Output
is reconstructed in bounded 64 KiB chunks rather than retaining a whole
expanded block. An RLE-heavy block therefore cannot size an output allocation
by its expanded length. The worker recycles at most eighteen chunk owners:
sixteen queued, one producing, and one held by the caller. Both queue directions
are disconnected before joining, including early drop and failed streams.

Like the reference, decoder thread requests above one select one entropy
parser overlapped with one persistent reconstruction worker; higher requests
do not create additional reconstruction lanes. Historical randomized blocks,
partial unused Huffman trees, and surplus dummy selectors are retained for
compatibility. The strict Rust streaming API reports malformed trailing or
partial next headers as errors after a complete stream.

The archive adapter selects single-stream mode, as 7-Zip's archive coder
does, ignoring any AES padding after the validated stream. Before
returning the last declared output bytes, it probes the codec's end so a
size-bounded file reader cannot hide a bad combined CRC, truncated trailer,
or surplus decoded bytes. Standalone decoding defaults to concatenated streams.

The archive pipeline grants that extra reconstruction worker only to a
BZip2 coder producing the folder's final output, when pipeline stages leave
a spare thread and no LZMA2 coder already owns parallel decoding. Every
other BZip2 coder runs single-threaded. This prevents nested codecs from
each claiming the archive's whole thread allowance.

For an eligible final-output BZip2 coder, an output callback computes the
container's file checksums on the reconstruction worker before publishing
each output chunk. The callback uses the archive's existing split/fold
protocol, so the delivering thread compares folded checksums without
rescanning output. AES or other transformations below BZip2 do not change
that final-byte domain. A transformation above BZip2 keeps final-byte
checksumming on the consuming thread. Chains containing LZMA2 do not pass
final-file splits through the BZip2 exception: a lower LZMA2 coder produces
intermediate compressed bytes, and its checksum coordinates cannot describe
the files. Those chains retain final-byte serial verification.

## Performance protocol

Build the `codec-bench` example in release mode before timing. It streams raw
input from a file and writes either to a file or, for output `-`, a discard
sink. Its arguments are `encode|decode LEVEL THREADS INPUT OUTPUT|-`
followed optionally by `[BLOCK_SIZE|-] [PASSES|-]`.
The historical Python tools are now preserved as Go commands in
`bench/bzip2-turbo-bench`. From that directory,
`go run . legacy-corpus DESTINATION --mib 32` generates byte-identical text, random,
mixed, and RLE-heavy inputs. `go run . legacy-compare --codec EXE --oracle EXE
--input FILE --results JSON` first verifies bytes, then records three-run
medians at 1/2/4/8 threads after one untimed warmup. Reference/candidate order
alternates for each repetition. It reports both median time and throughput
ratios, executable SHA-256 digests, host architecture, logical CPU count,
and configuration. Supply `--hardware`, `--revision`, and `--profile` to
record the hardware and source/build identities. `--block-size` and `--passes` override the codec
defaults for both implementations. Both commands require artifact destinations outside
the repository. The random corpus repeats a deterministic 1 MiB xorshift32
block, seeded with `0x12345678`; the period exceeds a BZip2 block.

On macOS, `--darwin-resources` wraps both competitors with `/usr/bin/time -l`
and records retired instructions, cycles, peak RSS, peak memory footprint,
context switches and faults for every measured repetition. RSS and footprint
are bytes. The report retains raw counter output, per-run values, medians and
maximum observed memory values. Its wall times include the same wrapper for
both competitors. On Linux, pair `perf stat` instructions/cycles with GNU
`time -v` peak RSS, converting its KiB value to bytes. Compare the reference
and Rust profiles on identical input and affinity; elapsed time alone cannot
identify extra work, cache stalls or scheduling contention.

Compare complete process time with these equivalent reference commands,
redirecting input and output in the same way for both processes:

```sh
7zz a -tbzip2 -mx=5 -mmt=4 -si -so stream.bz2 < input > output.bz2
7zz x -tbzip2 -mmt=4 -si -so < output.bz2 > decoded
```

Use the same preexisting input files and encoded streams for each competitor.
Verify exact encoded bytes and decoded bytes before timing. Include periodic
text, mixed binary, deterministic pseudorandom bytes, and long-run input; each
must be large enough to produce multiple blocks. Record corpus generation
recipe, byte count, digest, executable revision, release profile, hardware,
OS and thread count. Measure workers 1, 2, 4 and 8, at least three independent
runs each, and report the median wall time and raw-input throughput. Time on
an otherwise idle ARM64 Mac and x86-64 host, with no concurrent builds or other
benchmark runs. Linux x86 profiling compares both executables with `perf`, using
identical input and CPU affinity. Record background load and CPU governor;
do not change unrelated services or system settings for a measurement. Keep
generated corpora and raw results outside the repository. Filesystem output and
sink output are separate measurements. Acceptance requires every unrounded
candidate/reference median time ratio to be at most 1.0, with no tolerance.
Report peak RSS differences in absolute MiB as well as ratios. A bounded
increase of a few MiB is acceptable; this does not relax the timing gate.
The codec's live-allocation estimate and whole-process peak RSS are different
quantities, and both must be accounted for.
Profiles and counters collected under external load are diagnostic evidence,
not a substitute for the final timing gate.

## Verification status

The 2026-11-04 checkpoint passes 780 ARM64 workspace native tests with eight
expected skips, plus the separately enabled pinned-reference archive test.
Formatting, workspace all-target/all-feature strict Clippy, doctests and the
wasm host-crypto build pass. The exact merged codec passes 100 Linux release
tests with no skips and strict Clippy, including both native TT word widths.
Three bounded portable ownership/refusal tests and the compact/paired TT
oracle pass under pinned Miri; these do not verify native FFI.

The final archive-budget regression initially omitted the existing 64 KiB
compressed-input charge. Source inspection confirmed the unchanged model is
8 MiB for the codec plus that buffer; the corrected test verifies rejection
below this total and maximum-block decoding at the total for one/eight requested
threads. The full 780-test rerun includes it. An earlier isolated clamp sweep
reported one process-leak annotation without a retained per-test log. Logged
reruns and the final sweep did not reproduce it; its original cause is unresolved.

Full performance parity is **not established**. This is an implementation checkpoint,
not release acceptance. The complete zero-regression matrix remains open;
the final Linux smoke results were not collected after the host was reclaimed.
All prior candidates, measured binaries, raw samples and failed experiments
are preserved in external evidence archives.

The final merged ARM executable, SHA-256
`e9a59af974f3fb9d010a21e02047d0e319bdcfda628ad1358bbf2c197614ec3c`,
passed all six closeout smoke cases with byte verification and three alternating
timed repetitions. Reference ratios were 0.908112/0.932144 for source decoding
at one/two threads, 0.963914 for random decoding at two threads,
0.972909/0.901367 for random encoding at levels one/five with eight threads,
and 0.934165 for periodic level-five encoding on one thread. The largest
matched decoder RSS excess in this smoke was 4.984375 MiB. These six cases
do not replace the full matrix or supersede the larger observed RSS excess below.

This round integrates the 900,000-entry rank array only on macOS ARM64;
other targets retain the power-of-two array. Six level-five ARM encoder
comparisons all beat 7-Zip: source ST/8-thread ratios 0.7591/0.8425,
random 0.8424/0.8970, and periodic 0.9396/0.9302. Eight-thread RSS differences
were −0.83, +2.00 and −1.36 MiB, respectively. Periodic encoding costs 3–4%
against the preserved larger-array Rust variant while remaining faster than
the reference. Other block-size options have correctness coverage but no
new RSS qualification in this round.

The generic paired-link/pool decoder passed eight targeted ARM comparisons,
with reference time ratios from 0.45121 to 0.96856. Its largest matched
individual RSS excess was 5.15625 MiB; this is an observation, not a relaxed
memory criterion. Allocation profiling verified output allocations fell from
1,044 to 14 on random 64 MiB input. Compact links remain the default off macOS
ARM64 and are explicitly selected for finite archive budgets.

The resumable encoder and reusable parser candidates remain isolated.
Coarse encoder yields improved source input but did not close random
level-one/eight-thread parity; traces show workers without jobs while the
caller retains unfinished work. Parser reuse regressed long-source x86 MT
from 0.9917 to 1.0230 times reference time. Neither candidate is selected.

The measurements below record earlier stages and are not current-source gates.

Bulk literal admission keeps the existing slot scheduler but copies bounded
literal spans and updates RLE state once per span. CPU profiles and assembly
identified the former per-byte state stores; separate lifecycle traces measured
input filling falling from roughly 66–74 ms to 25–30 ms on the source corpus.
All 14 targeted level-five ARM timing comparisons beat the reference, with
byte-identical output. These are targeted diagnostics, not the full gate.
Smaller rank capacities then removed the level-one eight-thread RSS excess:
roughly 27.5 MiB above the reference became slightly below it on both source
and random input. Random level-one encoding at eight threads still measured
1.04365 times reference time. Random level-five eight-thread RSS also remains
an open issue. Each intermediate binary, source and measurement is preserved.

The ordered output carry now packs eight bytes at a time using explicit
big-endian words, with the existing scalar tail. Independent logical-bit and
failing-sink tests cover every alignment and output-buffer boundary. In its
paired diagnostic, random level-one eight-thread time fell from 1.0472 to
1.0313 times reference time; parity remains unmet. Lifecycle traces measured
ordered acceptance falling from 70–94 ms to 57–64 ms. The caller can still
hold up ready earlier blocks while finishing its own later compression job.

Matched ARM decoder allocation captures found exactly one TT allocation,
75 last-column allocations and 1,044 output-chunk allocations on random
64 MiB input. Compact decoder peak RSS varied from 9.39 to 13.59 MiB;
the isolated paired-link candidate varied from 12.80 to 14.30 MiB, against
8.72–8.73 MiB for the reference. The exports establish allocation churn and
final-live state, not allocation lifetime overlap or heap high-water.
These captures preceded bounded recycling. Paired links improved the
targeted ARM timings; their x86 long-source result
regressed alongside increased last-level-cache misses. All candidates and
raw measurements remain preserved outside the repository.

A five-run ARM64 source-input diagnostic compared the preserved encoder,
initial slots, inline writer and external-flags stages. For the last stage,
executable SHA-256
`8531b4c7618d4114e08fbdbff18de2e3c9b87bc153ed264c35973f7d198c40f6`,
the median time ratios to 7-Zip were 0.7607421230349886 at one thread and
1.0027164915403977 at eight. The latter fails the strict timing criterion.
Median peak RSS was 11.0625 and 75.796875 MiB, respectively, versus 12.03125
and 72.375 MiB for the reference. At eight threads, the external-flags stage
retired 23.226 billion instructions versus 25.988 billion for initial slots
and 21.197 billion for the reference. Encoded bytes matched throughout.
Background load makes these diagnostic results, not acceptance evidence;
all intermediate implementations and measurements remain preserved.

The earlier integrated aligned-TT revision passed 684 ARM64 native tests with eight
expected skips and 51 standalone Linux release tests against the pinned
reference. Four focused portable ownership tests passed separately under
Miri; this does not verify Linux FFI. Workspace and decode-only strict Clippy,
formatting, parent doctests and the wasm host-crypto build passed. The native
Memcheck/sanitizer selections are configured in CI but were not run locally.

The revision before Linux aligned TT storage, including `crc-turbo`, adaptive TT scatter,
the carried-run loop and architecture-selected MTF, plus the encoder carry
reservation fix, passed 676 native tests with eight expected skips. The pinned
reference release suite passed 44 tests. Strict Clippy with warnings denied,
formatting, parent doctests and the wasm host-crypto feature build passed.
Source review against the preserved `crc-combined` prototype found no algorithm changes in
the integration: TT construction moved into `decoder/tt.rs`, with names and
visibility adjusted; the decoder loop and its boundary tests were retained.

The zero-regression performance gate remains open. Current-source memory and
performance results must be reviewed separately; these correctness and lint
results do not establish performance parity. The completed 64-comparison Mac
diagnostic matrix met the numeric timing criterion in 50 comparisons and
failed it in 14. Background load prevents treating this as final acceptance.
It measured the source before the x86 MTF selection and encoder carry-reserve
fix, with codec executable SHA-256
`6e6de41ac7d5f125648235fcc39dc87f8fa4bc494893a00f89fac8558c0207bf`.

At level five, median peak RSS differences against 7-Zip were:

| Corpus / operation | 1 thread | 2 threads | 4 threads | 8 threads |
| --- | ---: | ---: | ---: | ---: |
| Periodic 64 MiB encode | +5.05 MiB | +15.41 MiB | +17.39 MiB | +28.34 MiB |
| Random 64 MiB encode | +16.95 MiB | +35.89 MiB | +50.53 MiB | +48.20 MiB |
| Source 40.31 MiB encode | +20.33 MiB | +36.97 MiB | +48.36 MiB | +56.92 MiB |
| Periodic 64 MiB decode | -2.31 MiB | +3.11 MiB | +1.95 MiB | +4.19 MiB |
| Random 64 MiB decode | -0.39 MiB | +5.64 MiB | +4.75 MiB | +4.58 MiB |
| Source 40.31 MiB decode | -0.39 MiB | +3.62 MiB | +2.67 MiB | +2.31 MiB |

The encoder's tens-of-MiB differences exceed the small-increase allowance;
bounded ownership alone does not resolve them. Raw samples, counters, source
identities and intermediate improvements are preserved outside the repository.

Paired ARM allocation traces of the later frozen executable
`ea5af8a18efe95bbd2b16f175c67aadbf9daf298b64c6d291faefc5885242230`
recorded 1,320,982,464 cumulative heap bytes in 15,497 allocations for the
40.31 MiB source corpus at eight threads. The reference recorded 97,618,544
bytes in 4,209 allocations. These are allocation volumes, not peak RSS.
The reference made eight sort-buffer and eight worker-arena allocations;
Rust repeatedly allocated rank, order, transformed-byte and symbol buffers.
The exported statistics do not establish transient live high-water or
allocator-retained RSS. Task-only profiling copies had identical machine-code
sections and debugger entitlements; acceptance binaries were unchanged.

Source inspection explains the ownership difference: `CThreadInfo::Alloc`
and `ThreadFunc` retain and reuse per-worker storage through input, MTF,
recursive trials and ordered delivery. The frozen Rust revision allocated
those roles separately and admitted additional queued blocks. ARM disassembly confirms the
rotated-rank mask lookup and vectorized rank updates are present; missing
vectorization is not an explanation for the memory difference.

The first reusable-slot executable
`71a1bc5d008fc6bd4468da17ab920db72b16418112b0ab008f5233cf18924977`
reduced that same allocation workload to 90,850,960 cumulative heap bytes
and 1,250 allocations. Separate five-run source-corpus measurements gave
11.06 MiB median peak RSS at one thread and 75.94 MiB at eight, versus
12.05 and 72.41 MiB for the reference. However, it retired about 25% more
instructions than the frozen Rust encoder and regressed throughput. CPU
samples and assembly located additional group-metadata scanning and
outlined writer/admission calls. The subsequent external-flags sorter and
writer changes are separately preserved and measured; allocation success
does not imply timing acceptance.

Fresh ARM CPU samples put random-data parsing chiefly in packed MTF updates
and source-data MT chiefly in the dependent TT reconstruction loop. CRC is
not the dominant sampled region. The earlier Linux packed-word path retired
fewer instructions than the reference but recorded substantially more branch
misses; hardware branch histories identified its generated MTF remainder and
unrolled loops. The x86 path now stores MTF bytes directly, uses paired word
carries below index 32, and uses the platform overlapping copy above it.
Measured source-data indices fell below 32 in 94.31% of updates, compared with
12.15% for random data. Explicit little-endian conversions preserve byte order
without alignment or pointer assumptions. ARM retains the packed paired path:
the byte-storage alternatives did not consistently improve its measurements.
The prior reverse-word, forward-word, byte-copy and short-prefix variants remain
preserved with their source, compiled assembly and measurements.

Five-run Linux diagnostics for the short-32 candidate improved all six measured
cases over the prior production decoder. Random ST retired 2.230 billion
instructions and had 12.733 million branch misses, versus 3.024 billion and
32.240 million before. Its source-input time ratios to the reference were
0.9950338059294791 at one thread and 1.0194286070413814 at two; the latter
still fails parity. Peak RSS remained below the reference in all six cases.
`byte_mtf_preserves_evolving_reduced_alphabets_and_unused_tail` independently
checks persistent state, all word boundaries and unused bytes; the existing
every-position packed-state oracle also compares the byte path.

CPU-only traces cannot attribute unsampled intervals to channel waits. The
generic reader's non-Send borrows and producer-thread checksum callbacks
also prevent silently reversing parser/reconstruction roles to match the
reference. Architecture changes require both measured attribution and those
ownership contracts, rather than percentage-only timing experiments.

The earlier CRC-only revision passed 666 native tests with eight expected
skips and its 39-test standalone suite. The results below describe the still
earlier initial baseline before the CRC backend change; they are not a final
verdict on the current source.

All eight standalone conformance tests passed for that baseline in a release build on ARM64 macOS
against 7-Zip built from the exact revision above: default levels 1–9,
byte-exact encoder outputs at workers 1/2/4, short-read decoder outputs at
workers 1/2/4/8, malformed streams, and a historical randomized block.
Additional passing coverage includes incomplete/dummy Huffman trees,
concatenation/trailing partial headers, and large periodic/RLE boundaries.
The explicit-option test also passed: block sizes 100,000, 200,000 and 900,000;
pass counts 1, 2, 3, 7 and 10; and workers 1/4, on a generated mixed-data
input crossing the smaller block boundaries.

The initial-baseline ARM64 native sweep passed 657 tests (eight pre-existing or explicitly
oracle-gated skips); the archive oracle test was also run explicitly and passed.
Its standalone release suite passed all 34 tests. Doctests passed three tests
with one existing ignored test. Formatting, workspace all-target/all-feature
Clippy with warnings denied, decode-only Clippy, strict rustdoc, and the wasm
host-crypto feature-set check passed. Native validation used Rust 1.97.1 and
Nextest 0.9.143.

### ARM64 baseline performance

Measured on an Apple M5 Max with 18 logical CPUs, 128 GiB RAM and macOS 26.6.2.
The candidate uses the workspace release profile: fat LTO and one codegen unit.
Acceptance requires zero regression: every candidate/reference median wall-time
ratio must be at most 1.0. These baseline measurements do not pass that gate;
the worst decoder ratio is 1.0253. Each row below gives the baseline ratios at 1/2/4/8
threads; lower is faster. Every case includes byte-exact encoding and decoded
SHA-256 verification before one warmup and three alternating timed repetitions.

| Corpus / level | Encode 1 / 2 / 4 / 8 | Decode 1 / 2 / 4 / 8 |
| --- | --- | --- |
| Periodic, 64 MiB / 5 | .943 / .875 / .880 / .897 | .994 / 1.005 / 1.013 / 1.002 |
| Random, 64 MiB / 5 | .772 / .786 / .787 / .791 | 1.025 / .965 / .952 / .977 |
| Runs, 64 MiB / 5 | .695 / .710 / .708 / .712 | .295 / .320 / .326 / .326 |
| Source, 40.31 MiB / 5 | .716 / .723 / .712 / .721 | .990 / .992 / .990 / .985 |
| Source, 10.08 MiB / 1 | .751 / .747 / .754 / .805 | .990 / .785 / .779 / .773 |
| Source, 10.08 MiB / 9 | .824 / .826 / .820 / .837 | .985 / 1.001 / .978 / .992 |
| Random, 64 MiB / 1 | .833 / .819 / .828 / .876 | 1.001 / .979 / .984 / .988 |
| Random, 16 MiB / 9 | .876 / .880 / .884 / .921 | 1.000 / .968 / .972 / .939 |

The measured source identity is
`0ff8aab201837ad070fc12c5b626653026a59ea54d7a67177ebae1d10cf5f94b`;
the ARM64 executable SHA-256 is
`0887b51cbea1af5855c6d227bdd111d4b4a73d4432e5a6ebe06bb233ad1aa451`.
The identity manifest records hashes of the four codec source files, benchmark
entry point, and two Cargo manifests. The implementation is uncommitted on
`feature/bzip2-turbo`, based on `d6efe9c262b09dc73fe818d947cfb60e838db17c`.

For these measurements, source is `git archive` of `C` and `CPP` at the pinned
7-Zip revision (10,567,680 bytes); the larger source corpus concatenates four
copies. Random input is 16 MiB from Python's
`random.Random(0xB21F00D).randbytes(16 * 1024 * 1024)`; its 64 MiB form repeats
that buffer four times. Periodic input repeats
`BZip2 block sorting and streaming pipeline conformance.\n` to exactly 16 MiB,
then concatenates four copies of that buffer.
Runs input is 64 MiB of byte `0x5a`. Raw result files record every input digest,
executable digest, repetition and command configuration. These are sink-output
process timings; they do not measure destination filesystem throughput.
