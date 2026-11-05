# Native decoder improvements

The native `Decoder` changes in runtime commit `8a2188e6b667` were selected
from paired measurements against the pinned 7-Zip 26.04 reference. They do
not establish full performance parity. The compatibility adapters retain their
separate incremental parser; native-parser timings do not measure that API.

## Follow the complete stream

Single-threaded decoding parses one compressed block, constructs its inverse-BWT
lookup table, reconstructs bounded chunks, updates the CRC and supplies bytes
through `Read`. CRC verification completes at the block boundary; a later
CRC error cannot retract a prefix already returned to the caller. Multithreaded decoding overlaps parsing with one
reconstruction worker. Increasing the requested thread count above two does
not create more reconstruction workers.

The Graviton baseline profile placed most of the MKV difference in entropy
parsing, even though inverse-BWT reconstruction was also a large hotspot in
both implementations. Removing reconstruction instructions did not consistently
reduce elapsed time. The accepted changes therefore address parsing control
flow, table construction and storage lifetime together.

## Retained changes

- Keep active bits at the high end of the reservoir, allowing a constant shift
  for the fast Huffman index. Preserve the existing 32-bit refill policy and
  reader/error behavior, including fragmented input and `WouldBlock`.
- Use the reference's forward word-at-a-time MTF carry loop on Linux ARM64.
  Native differential tests compare every position and evolving MTF state.
  Other platforms retain their previous MTF selection.
- Carry the next last-column byte through scalar inverse-BWT table construction,
  as in `DecodeBlock1`. An explicit final element avoids reading a sentinel
  outside the input; the existing long-run specialization remains intact.
- Reuse initialized output storage and track its valid prefix separately.
  Unobserved single-threaded decoding uses a 1 MiB buffer. Callback and worker
  chunk boundaries remain unchanged. A failed block cannot expose stale bytes.
- Store validated Huffman selectors as bytes, retaining validation and input
  consumption even beyond the selector storage cap.
- Retain alphabet, selectors, code lengths and Huffman table allocations across
  blocks. Rebuilding clears canonical metadata, symbols and fast-table sentinels.
  Published blocks own their data and never borrow this scratch storage.

These are safe Rust changes. The decoder's existing unsafe-code prohibition
remains in place; no unchecked lookup experiment or experimental CI change was
integrated. Public signatures and dependency versions are unchanged.

## Memory and correctness

On 64-bit targets, the conservative compact single-threaded requested-allocation
subtotal is 6,223,996 bytes, below the documented 6 MiB estimate. This counts the
retained tables and worst-case 900,000-byte last column. It excludes the reader,
stack, allocator metadata and other exclusions stated by the public memory
estimate; it is not a whole-process RSS promise. Byte selectors offset the
larger single-threaded output buffer. Worker output ownership remains bounded.

Tests compare the high-aligned reservoir against the earlier implementation,
including exact read consumption and error outcomes. Additional tests cover
large-to-small and incomplete table rebuilds, allocation reuse across concatenated
members, selectors beyond the storage cap, short and empty reads, repeated EOF,
CRC rejection and callback failure. Existing interoperability tests verify
reference bytes and malformed-stream handling.

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

See the [README measurements](../README.md#benchmarks) for the source identities,
output method and remaining workload-specific gaps. Earlier diagnostic runs
used Rust's in-memory sink while 7zz wrote to the null device. The final
four-way follow-up uses the operating-system null device for every engine;
measurements from those two protocols must not be combined.

## Additional block-size checks

The signed runtime was compared with the previous turbo runtime at `92a4610`
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
regression claim. Correctness passed, but the strict zero-regression and
universal 7zz parity goals have not been met. This change preserves the measured
improvements and records the remaining work.
