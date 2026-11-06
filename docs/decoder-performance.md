# Native decoder improvements

The native `Decoder` changes in runtime commit `8a2188e6b667` were selected
from paired measurements against the pinned 7-Zip 26.04 reference. The
decode-parity follow-up measured below brings native decoding to 7zz parity or
better on every measured x86-64 case and on most AArch64 cases, with the
remaining AArch64 gaps listed. The compatibility adapters retain their
separate incremental parser; native-parser timings do not measure that API.

## Follow the complete stream

Single-threaded decoding parses one compressed block, constructs its inverse-BWT
lookup table, reconstructs bounded chunks, updates the CRC and supplies bytes
through `Read`. CRC verification completes at the block boundary; a later
CRC error cannot retract a prefix already returned to the caller. Multithreaded decoding overlaps parsing with one
reconstruction worker. Increasing the requested thread count above two does
not create more reconstruction workers. As in `RunScout`, the parser queues
the next block as soon as it is parsed, so the worker can start it at a block
boundary without waiting for the caller to observe the previous block's end.
When the worker has not yet taken the block in flight, the parser waits for
the one-slot queue instead of holding the successor back; the worker is then
between blocks, so that wait cannot depend on the caller. The pipeline still
holds at most two parsed blocks at once, counting the one being reconstructed.
Dropping the decoder discards a block still waiting in the queue before the
worker builds its table.

The Graviton baseline profile placed most of the MKV difference in entropy
parsing, even though inverse-BWT reconstruction was also a large hotspot in
both implementations. Removing reconstruction instructions did not consistently
reduce elapsed time. The accepted changes therefore address parsing control
flow, table construction and storage lifetime together.

## Retained changes

- Keep active bits at the high end of the reservoir, allowing a constant shift
  for the Huffman window. Preserve the existing 32-bit refill policy and
  reader/error behavior, including fragmented input and `WouldBlock`.
- Decode parser symbols with the structure of 7-Zip's `NHuffman::CDecoder`
  (HuffmanDecoder.h `Build` and `Z7_HUFF_DECODE_BASE`, as BZip2Decoder.cpp uses
  it): each symbol reads one 20-bit window. If its top `TABLE_BITS` bits (9, as
  in 7-Zip) fall below the short-code bound, a direct table gives the code
  length and symbol. Otherwise the window is compared against per-length code
  limits, left-aligned to 20 bits, to find the length, and a per-length position
  bias indexes the long-code symbols, which are stored in canonical order after
  the table region. There are no bit-serial reads. All arrays are fixed-size and
  rebuilt in place per block. A window past the last code of an incomplete tree
  is rejected as invalid; oversubscribed trees are still rejected at build time.
  Every symbol needs 20 available bits, which a valid stream always has because
  the end-of-stream footer follows the last block; a stream cut closer than that
  is a truncation error. The suspendable incremental decoder keeps its
  bit-serial canonical walk.
- Use the reference's forward word-at-a-time MTF carry loop on Linux ARM64.
  Native differential tests compare every position and evolving MTF state.
  Other platforms retain their previous MTF selection.
- Scatter inverse-BWT links as `DecodeBlock1` does, with each bucket counter
  loaded and stored through one plain address register. The ordinary scatter
  builds a table of references to the 256 counter cells once per block, walks
  the last column two positions per step, and fetches the next pair's counter
  references one step ahead so the load-increment-store chain carried through
  repeated buckets never waits on that lookup; equal buckets within a pair
  share one cell and update in order, and no byte is read beyond the input.
  The link and last-column slices are bound to one length and each bucket
  start is clamped to the final index. For a parser histogram the clamp is the
  identity; it compiles to a conditional select, so the loop keeps only its
  back-edge branch with no per-byte bounds checks. The counter advances
  unclamped. Generated x86-64 code went from 14 to 12 instructions per
  position with scaled-index counter addressing replaced by register-indirect
  addressing, which on the Intel host closed the level 5 and 9 scatter gap
  (logs.l5 1T from 1.070 to 1.016 of the 7zz time); AArch64 code went from 13
  to 12.5 instructions per position with no timing change. The existing
  long-run specialization remains intact, and the paired successor pass clamps
  its source index the same way.
- Reuse initialized output storage and track its valid prefix separately.
  Unobserved single-threaded decoding uses a 1 MiB buffer. Callback and worker
  chunk boundaries remain unchanged. A failed block cannot expose stale bytes.
- Store validated Huffman selectors as bytes, retaining validation and input
  consumption even beyond the selector storage cap.
- Retain alphabet, selectors, code lengths and Huffman table allocations across
  blocks. Rebuilding clears canonical metadata, symbols and the CDecoder limit,
  position and lookup arrays.
  Published blocks own their data and never borrow this scratch storage.

## Compact reconstruction loop

The compact kernel follows the loop shape of `CSpecState::Decode`. Per output
byte it keeps only the output index, the chained position, the previous byte
and the reference's signed `reps` countdown live; the previous-byte test
updates `reps` with a conditional select. Because each symbol other than a
repeat count yields one byte, the remaining-symbol count is carried as the
output index where the block ends, so the inner loop has a single exit
compare against the nearer of the output and block limits. Repeat counts
leave the inner loop for a cold run-fill helper, and loop state is written
back only when a read returns. The link load keeps its safe bounds check
against a register-held length. The CRC is still computed in one batch per
read, runs still carry across reads, and decoded bytes and errors are
unchanged; a unit test compares every read result, output buffer and state
with the previous loop. Generated AArch64 and x86-64 code was inspected for
this shape, including the absence of stack reloads in the loop. Its timings
are part of the measured results below.

These are safe Rust changes; no unsafe code was added to the decoder and no
experimental CI change was integrated. Public signatures and dependency
versions are unchanged.

## TT storage

`CDecoder` allocates `_counters` once, at the format maximum, through
`BigAlloc`: TT words and the byte-mode last column share one allocation that,
on Linux, is aligned to 2 MiB and advised with `MADV_HUGEPAGE`. Every block of
every level therefore chases its links through huge-page-backed memory.

On Linux x86-64 and AArch64 the streaming `Decoder` follows that policy with
separately owned buffers. Every TT and every last column is rounded up to
whole 2 MiB pages (at least one), aligned to 2 MiB and advised with
`MADV_HUGEPAGE`, whatever its size. This rounding is a deliberate superset of
`BigAlloc`, which aligns only allocations above 1 MiB and rounds a size up to
a 2 MiB multiple only when its remainder exceeds 1 MiB. Each buffer is its
own anonymous private `mmap`, trimmed to the 2 MiB-aligned extent and released
with `munmap`, so neither Rust's global allocator nor the C `malloc` the
process links (glibc, mimalloc, jemalloc or another) is involved in its
placement or release. Nothing is pre-filled, as in `BigAlloc`: the advice is
applied before any access, and the kernel supplies zeroed pages on first
touch. Compact links take 2 MiB for levels 1–5 and 4 MiB for levels 6–9; a
last column of any level fits one 2 MiB page. The TT is sized once at the
stream's declared block limit and is never shrunk or reallocated while it is
large enough; a later member with a larger level grows it once. Last columns
come from a small recycled pool instead of a fresh allocation per block: one
owner when single-threaded, returned as soon as its TT is built, and at most
two when pipelined, each returned with its block's completion message. The
kernel honours the advice only when transparent huge pages are set to
`madvise` or `always`; with `never` these buffers use ordinary 4 KiB pages, as
before.

The bzip2-compatible API (`Decompress`, the `read`, `bufread` and `write`
decoders, and `MultiBzDecoder`) gets the mapped TT only: it still builds each
block's last column in a plain `Vec`.

Other targets keep exact-size `Vec` storage, and the recycled column is a
`Vec` there. Each `Vec`-backed buffer tracks its logical length separately
from its initialized extent, so shrinking and regrowing within the largest
length it has held neither zero-fills nor exposes uninitialized words; only
growth past that mark zero-fills the new words, once. The parser stores each
byte at a bounded index into the column, with no per-byte capacity check
beyond the declared-size test.

## Memory and correctness

On 64-bit Linux x86-64 and AArch64, the conservative compact single-threaded
requested-allocation subtotal is 7,421,148 bytes, below the documented 8 MiB
estimate: a 4 MiB level-9 TT, one 2 MiB last-column page, the 1 MiB output
buffer, the 32 KiB input buffer and 48,348 bytes of retained entropy tables.
On other 64-bit targets the exact-size TT and 900,000-byte last column give
5,629,692 bytes, below the 6 MiB estimate. With two threads, the Linux
subtotal is 9,649,372 bytes, below 12 MiB: the TT, two last-column pages,
eighteen 64 KiB worker output chunks, the input buffer and the tables.
These exclude the reader, stack, allocator metadata and other exclusions
stated by the public memory estimate; they are not a whole-process RSS
promise. Byte selectors offset the larger single-threaded output buffer.
Worker output ownership remains bounded.

Tests compare the high-aligned reservoir against the earlier implementation,
including exact read consumption and error outcomes. Additional tests cover
large-to-small and incomplete table rebuilds, allocation reuse across concatenated
members, selectors beyond the storage cap, short and empty reads, repeated EOF,
CRC rejection and callback failure. Existing interoperability tests verify
reference bytes and malformed-stream handling.

## Measured results

The decode-parity follow-up (the reconstruct loop, clamped and
register-addressed scatter, run-ahead, huge-page TT storage mapped through
`mmap`, the CDecoder Huffman structure and the worker cancellation changes)
was measured on two Linux hosts, a Graviton3 (Neoverse V1) instance and an
Intel Xeon (Sapphire Rapids) instance, against 7zz 26.04 built from the pinned
source on the same host. Each entry is the median of nine interleaved runs of
the complete process pinned to the same cores, reading from the page cache and
writing to the null device, with 1T meaning one decoding thread and 2T the
run-ahead worker. Streams were produced by 7zz at levels 1, 5 and 9 from the
canonical fixtures. Every decode was byte-compared with the original before
timing. Ratios are this crate's time over 7zz's; below 1.0 is faster.

| case | Graviton3 1T | Graviton3 2T | Intel 1T | Intel 2T |
| --- | --- | --- | --- | --- |
| logs L1 | 0.97 | 1.00 | 1.01 | 0.96 |
| json L1 | 0.97 | 0.97 | 1.01 | 0.83 |
| film L1 | 1.07 | 1.02 | 0.72 | 0.52 |
| program L1 | 1.01 | 0.93 | 0.85 | 0.63 |
| logs L5 | 0.98 | 1.03 | 1.02 | 1.00 |
| json L5 | 0.99 | 1.01 | 1.02 | 1.03 |
| film L5 | 1.04 | 1.01 | 0.74 | 0.57 |
| logs L9 | 0.98 | 1.04 | 1.02 | 0.99 |
| json L9 | 0.99 | 1.00 | 1.01 | 1.01 |
| film L9 | 1.06 | 1.00 | 0.81 | 0.89 |
| repeated L9 | 0.56 | 0.58 | 0.79 | 0.84 |

Relative to the previous main build on the same hosts, the logs and json
level 1 cases improved by 13 to 19 percent on Graviton3 and by 18 to 21
percent at 2T on Intel, and the Intel level 5 and 9 cases by 1 to 13 percent.
TLB refill counts
now match 7zz on both hosts (previously 150 times higher on Graviton3), and the
`mmap` storage path makes these results independent of the process allocator.

Remaining gaps, all on Graviton3: the film cases at 1T are 4 to 7 percent
behind, entirely in the parser, where this crate executes fewer instructions
than `ReadBlock2` but takes about 22 percent more branch misses; the CDecoder
Huffman port did not change that count, so the source is elsewhere in the
parser and is not yet diagnosed. The level 5 and 9 cases at 2T are 1 to 4
percent behind, tied to per-chunk handoffs between the parser and the worker
(about seven times the context switches of 7zz's two-thread path). Intel
level 5 and 9 cases at 1T sit 1 to 2 percent behind, within run-to-run noise.

## Evidence and limits

Graviton measurements use complete processes and include first-use allocation
and initialization. Generated code confirmed that persistent scratch reduced
the parser frame from 13,680 to 4,880 bytes in the measured Linux ARM64 build;
this is not a guarantee for other compilers or targets. Mac ARM regression
checks used the same five canonical fixtures and verified decoded bytes before
six balanced timing rounds.

Fixed last-column initialization, unchecked TT lookup and group-local entropy
state were not integrated. The first two did not produce a reliable overall
win; the last failed its intended assembly check because per-symbol state
traffic moved to the stack instead of registers. Their patches and raw evidence
remain preserved outside Git.

See the [detailed measurements](benchmark.md) for the source identities,
output method and remaining workload-specific gaps. Earlier diagnostic runs
used Rust's in-memory sink while 7zz wrote to the null device. The `8a2188e`
decoder-only follow-up used the operating-system null device for every engine.
The current `f6cb7d2` full-suite results are reported separately; samples from
different sources or protocols must not be combined.

## Historical decoder regression checks (8a2188e)

Before the current full-suite run, runtime `8a2188e` was compared with the
previous turbo runtime at `92a4610`
and 7zz on all five canonical fixtures at 100,000-byte (L1) and 900,000-byte
(L9) blocks, with one and two threads. All 60 pre-timing output hashes matched;
360 measured runs covered 20 scenarios. Six runs per engine and scenario used
uniform null-device output. These are wall-time differences from same-batch
medians; negative is faster and positive is slower.

| Case | vs prior turbo | vs 7zz |
|---|---:|---:|
| JSON L1 1T | -3.18% | +12.41% |
| JSON L1 2T | -2.49% | +16.18% |
| JSON L9 1T | -1.81% | -0.11% |
| JSON L9 2T | -0.48% | +0.19% |
| Logs L1 1T | -3.17% | +14.77% |
| Logs L1 2T | -2.24% | +22.80% |
| Logs L9 1T | -2.61% | +0.22% |
| Logs L9 2T | -0.28% | +0.63% |
| Repeated L1 1T | +0.41% | -1.70% |
| Repeated L1 2T | +0.11% | +3.07% |
| Repeated L9 1T | +0.41% | -41.35% |
| Repeated L9 2T | +0.19% | -38.91% |
| ELF L1 1T | -7.30% | +6.19% |
| ELF L1 2T | -7.97% | -7.12% |
| ELF L9 1T | -5.25% | +2.01% |
| ELF L9 2T | -6.26% | -3.62% |
| MKV L1 1T | -8.09% | +10.61% |
| MKV L1 2T | -8.66% | +1.01% |
| MKV L9 1T | -4.67% | +4.11% |
| MKV L9 2T | -0.13% | +0.56% |

Repeated-text medians increased by at most 0.32 ms. Small differences and
outliers remain in the raw evidence; they are not rounded away into a zero
regression claim. Correctness passed, but those checks did not meet the strict
zero-regression or universal 7zz parity goals. These historical comparisons
preserve the measured improvements and remaining work.
