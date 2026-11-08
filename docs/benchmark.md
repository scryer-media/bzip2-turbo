# Benchmark details

> **Version note.** These are older results for 0.1 (source `f6cb7d2`),
> measured on Linux Graviton3 and Intel Xeon hosts, not on the hardware behind
> the README's 0.2 tables. They describe 0.1's behaviour: a native decoder of
> at most two threads, and an encoder whose thread budget included the calling
> thread. In 0.2 the decoder runs N block workers, the encoder's `threads`
> counts compression workers, and level 9 defaults to one pass. These tables
> use explicit one-pass settings and have not been re-measured for 0.2.

Measured 2026-11-05 from the current signed source. Each host contains 60 settings: five fixtures, levels 1/5/9 and requested budgets 1/2/4/8. All engines use `level × 100,000`-byte blocks. Native turbo and 7zz use one optimization pass; the serial Rust/C references do not expose that pass option. All four encoders were cross-decoded by all four decoders before timing; native turbo/7zz encoded bytes matched. Every timed decoder reads the same 7zz stream. Four balanced measured rounds follow one warmup. Rates use actual original input bytes and per-fixture median wall seconds; MiB is 1,048,576 bytes.

At approximately 22:48 UTC, the scheduled shutdown timer on each task host was extended while its suite continued. This small administrative event is recorded in the evidence. All measured samples are retained; no codec run was restarted, filtered or rerun. Source, commands and codec settings were unchanged.

The output sink is `/dev/null` for every timed engine and operation. Wall time includes GNU time wrapper startup, codec startup and I/O. RSS is median peak codec-command RSS from GNU time's own child report (`gnu-time-child-v1`), not the Go parent's pre-exec high-water mark. CPU fields are reported at GNU time's hundredth-second precision. RSS is not aggregate simultaneous process-tree memory. Budgets are requests: native decoding uses at most two total threads; the pure Rust and C references remain serial at every comparison budget.

These are prior results: the harness used to time decode at every requested budget. From this change on it measures decode only at 1T and 2T, matching the native decoder's two-thread cap; 4T and 8T budgets still measure encoding and record decode as skipped scenarios. The 4T and 8T decode rows below are kept as measured.

Build receipts record Rust 1.97.1 with optimization level 3, fat LTO, one codegen unit and stripped executables, with no target-CPU or extra Rust flag override. The Go harness uses Go 1.23.12. 7zz 26.04 uses pinned SDK source `9128b80e3a471108678e2b3ec3c985861c8d7b0f`, GCC 13.3.0 and its stock makefile.gcc build. Serial references use bzip2 0.6.1 with libbz2-rs-sys 0.2.6, or the bundled static libbz2 1.0.8 backend.

Signed source revision: `f6cb7d2c02a9d988dc55f031a7f8ecce8eab8f8c`. Source content SHA-256: `bb1482244ade70cec0f81d222e8aa43bc559bc2c4534ac6b9bcf39c104606047`. Approved source archive SHA-256: `7af740b8bd9928cb609db9499bbdca9363e9755711e2fd13d5ceb759b5cf4cac`.

| Fixture | Actual bytes | SHA-256 |
|---|---:|---|
| Varied JSON | 16777216 | `f7858958c4a1f4a42ba26006a058fa41c657fcd7f2e2d321300d517398e9530e` |
| Repeated sentence stress | 16777216 | `8befb6b762230bc79630499c8086a8137f0a0078c688b51348115699180fbece` |
| Structured logs | 16777222 | `170a8ba5218e7c1f67a3ba9031daa904540196a8aa972425b19c03ae5931ecd1` |
| H.264/AAC Matroska | 16360253 | `a88533696bc7ff1bb9847af06ccf37aafa7c781ea7d94eefbd7c8ff10d34525a` |
| Linux executable | 3903413 | `5d993bf86e336c2c54ee67c0c6be15fc1418e4e16d581a87d256823f467bfa97` |

JSON uses varied values and recurring keys; logs are complete varied structured records. Repeated text is 71-byte sentence stress, not representative prose. Media is a 150-second H.264/AAC [Big Buck Bunny](https://peach.blender.org/about/) excerpt, Copyright 2008 Blender Foundation, [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/), trimmed from 30 seconds and remuxed to Matroska with replaced attribution metadata. Canonical media was generated with FFmpeg 8.1.1. ELF is a real Linux executable generated with Go 1.27.0. No input is padded or truncated to force a requested container/executable size. Canonical bytes are identical on both hosts; fixture receipts and build proofs retain provenance.

## ARM64

Host: c7g.2xlarge, Graviton3, 16 GiB RAM, Ubuntu 24.04.

The host exposes 8 allowed logical CPUs across 8 physical cores. Every contender uses the same affinity set at a given requested budget; physical cores are selected before SMT. Native decoder capacity is 1/2/2/2 total threads at requested budgets 1/2/4/8; the 7zz setting is a request, not a measured live-thread count. The 4T and 8T decode rows below are prior results that measured the same two-thread native pipeline as 2T.

| Requested budget | Allowed logical CPUs |
|---|---|
| 1T | `[2]` |
| 2T | `[2 3]` |
| 4T | `[2 3 4 5]` |
| 8T | `[2 3 4 5 6 7 0 1]` |

| Engine | Executable SHA-256 |
|---|---|
| turbo | `ba6070a76db0a7ad247243c58e665baa886d22a0814575ed262ebbd7533ced3e` |
| 7zz | `765161e82b3e43a23a4fcd2492723a8154d640aa0b4abf5823787978b3368a1f` |
| pure | `1b827f8cbde07bcb328cca02d49144c02c659272fcd624c2c5dd1ce80dce794d` |
| c | `df9ac406e1ff49ddf95900534054b8ca6acbacb3ac1108c8344ffb9a42492ca1` |

### Level 1, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 19.0 · 3.2 | 84.3 · 3.5 |
| JSON / 7zz | 12.9 · 6.9 | 92.8 · 9.7 |
| JSON / bzip2 (Rust) (serial) | 16.4 · 2.6 | 58.8 · 2.0 |
| JSON / bzip2 (C) (serial) | 15.3 · 2.5 | 55.8 · 2.0 |
| Repeated text / bzip2-turbo | 9.5 · 3.2 | 202.3 · 3.5 |
| Repeated text / 7zz | 4.1 · 6.8 | 198.8 · 9.6 |
| Repeated text / bzip2 (Rust) (serial) | 7.0 · 2.6 | 149.4 · 2.0 |
| Repeated text / bzip2 (C) (serial) | 5.5 · 2.6 | 134.7 · 2.0 |
| Logs / bzip2-turbo | 21.9 · 3.2 | 101.3 · 3.5 |
| Logs / 7zz | 13.9 · 6.9 | 114.7 · 9.7 |
| Logs / bzip2 (Rust) (serial) | 15.3 · 2.7 | 77.4 · 2.0 |
| Logs / bzip2 (C) (serial) | 14.3 · 2.5 | 69.7 · 2.0 |
| MKV / bzip2-turbo | 12.8 · 3.3 | 32.4 · 3.5 |
| MKV / 7zz | 10.2 · 7.0 | 35.7 · 9.7 |
| MKV / bzip2 (Rust) (serial) | 7.9 · 2.6 | 23.3 · 2.0 |
| MKV / bzip2 (C) (serial) | 9.5 · 2.5 | 27.6 · 2.0 |
| ELF / bzip2-turbo | 14.5 · 3.3 | 46.7 · 3.5 |
| ELF / 7zz | 11.5 · 7.0 | 49.6 · 9.7 |
| ELF / bzip2 (Rust) (serial) | 11.6 · 2.6 | 30.3 · 2.0 |
| ELF / bzip2 (C) (serial) | 12.3 · 2.5 | 32.8 · 2.0 |

### Level 1, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 37.4 · 4.5 | 133.8 · 2.8 |
| JSON / 7zz | 25.6 · 9.3 | 150.9 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 16.4 · 2.6 | 58.9 · 2.0 |
| JSON / bzip2 (C) (serial) | 15.3 · 2.5 | 55.7 · 2.0 |
| Repeated text / bzip2-turbo | 15.1 · 4.5 | 202.6 · 2.6 |
| Repeated text / 7zz | 8.1 · 9.0 | 209.1 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 7.0 · 2.6 | 149.4 · 2.0 |
| Repeated text / bzip2 (C) (serial) | 5.6 · 2.6 | 135.0 · 2.0 |
| Logs / bzip2-turbo | 42.8 · 4.5 | 140.2 · 2.7 |
| Logs / 7zz | 27.5 · 9.2 | 172.8 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 15.3 · 2.7 | 77.4 · 2.0 |
| Logs / bzip2 (C) (serial) | 14.3 · 2.5 | 69.7 · 2.0 |
| MKV / bzip2-turbo | 25.2 · 4.8 | 41.6 · 2.7 |
| MKV / 7zz | 20.3 · 9.4 | 42.0 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 7.9 · 2.6 | 23.3 · 2.0 |
| MKV / bzip2 (C) (serial) | 9.5 · 2.5 | 27.6 · 2.0 |
| ELF / bzip2-turbo | 27.7 · 5.0 | 65.1 · 2.9 |
| ELF / 7zz | 21.9 · 9.5 | 60.5 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 11.6 · 2.7 | 30.3 · 2.0 |
| ELF / bzip2 (C) (serial) | 12.3 · 2.5 | 32.8 · 2.0 |

### Level 1, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 72.8 · 7.0 | 133.3 · 2.8 |
| JSON / 7zz | 50.7 · 13.3 | 151.2 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 16.4 · 2.6 | 58.6 · 2.0 |
| JSON / bzip2 (C) (serial) | 15.3 · 2.5 | 55.9 · 2.0 |
| Repeated text / bzip2-turbo | 29.7 · 7.0 | 202.5 · 2.6 |
| Repeated text / 7zz | 16.1 · 13.3 | 209.3 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 6.9 · 2.6 | 149.6 · 2.0 |
| Repeated text / bzip2 (C) (serial) | 5.6 · 2.7 | 135.5 · 2.0 |
| Logs / bzip2-turbo | 82.6 · 7.1 | 140.7 · 2.7 |
| Logs / 7zz | 54.4 · 13.3 | 173.0 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 15.3 · 2.6 | 77.4 · 2.0 |
| Logs / bzip2 (C) (serial) | 14.3 · 2.5 | 69.8 · 2.0 |
| MKV / bzip2-turbo | 49.0 · 7.5 | 41.5 · 2.7 |
| MKV / 7zz | 40.2 · 13.8 | 42.0 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 7.9 · 2.7 | 23.3 · 2.0 |
| MKV / bzip2 (C) (serial) | 9.5 · 2.5 | 27.7 · 2.0 |
| ELF / bzip2-turbo | 50.4 · 7.8 | 65.2 · 2.9 |
| ELF / 7zz | 41.9 · 14.0 | 60.7 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 11.6 · 2.7 | 30.4 · 2.0 |
| ELF / bzip2 (C) (serial) | 12.3 · 2.5 | 32.9 · 2.0 |

### Level 1, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 133.9 · 12.1 | 129.4 · 2.8 |
| JSON / 7zz | 99.0 · 21.8 | 151.3 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 16.5 · 2.6 | 57.7 · 2.0 |
| JSON / bzip2 (C) (serial) | 15.3 · 2.5 | 55.8 · 2.0 |
| Repeated text / bzip2-turbo | 53.7 · 12.0 | 203.0 · 2.6 |
| Repeated text / 7zz | 31.5 · 21.7 | 209.4 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 7.0 · 2.6 | 149.7 · 2.0 |
| Repeated text / bzip2 (C) (serial) | 5.6 · 2.6 | 135.3 · 2.0 |
| Logs / bzip2-turbo | 149.3 · 12.0 | 140.8 · 2.8 |
| Logs / 7zz | 104.1 · 21.7 | 173.1 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 15.3 · 2.6 | 77.3 · 2.0 |
| Logs / bzip2 (C) (serial) | 14.3 · 2.5 | 69.8 · 2.0 |
| MKV / bzip2-turbo | 90.7 · 13.2 | 41.6 · 2.7 |
| MKV / 7zz | 77.0 · 22.9 | 42.1 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 7.9 · 2.7 | 23.3 · 2.0 |
| MKV / bzip2 (C) (serial) | 9.5 · 2.5 | 27.6 · 2.0 |
| ELF / bzip2-turbo | 80.3 · 13.3 | 65.2 · 2.9 |
| ELF / 7zz | 76.0 · 23.1 | 60.7 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 11.6 · 2.6 | 30.4 · 2.0 |
| ELF / bzip2 (C) (serial) | 12.3 · 2.5 | 32.5 · 2.0 |

### Level 5, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 15.4 · 7.0 | 42.2 · 5.4 |
| JSON / 7zz | 10.9 · 11.5 | 42.1 · 9.7 |
| JSON / bzip2 (Rust) (serial) | 15.0 · 5.2 | 32.7 · 3.5 |
| JSON / bzip2 (C) (serial) | 14.0 · 5.2 | 32.0 · 3.5 |
| Repeated text / bzip2-turbo | 5.6 · 6.6 | 340.4 · 5.4 |
| Repeated text / 7zz | 3.1 · 11.2 | 199.0 · 9.6 |
| Repeated text / bzip2 (Rust) (serial) | 5.3 · 5.7 | 148.2 · 3.5 |
| Repeated text / bzip2 (C) (serial) | 4.6 · 5.6 | 133.3 · 3.5 |
| Logs / bzip2-turbo | 18.0 · 6.7 | 68.1 · 5.4 |
| Logs / 7zz | 11.9 · 11.4 | 67.2 · 9.7 |
| Logs / bzip2 (Rust) (serial) | 13.2 · 5.2 | 51.4 · 3.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 5.3 | 47.7 · 3.5 |
| MKV / bzip2-turbo | 12.2 · 7.5 | 23.2 · 5.4 |
| MKV / 7zz | 9.5 · 12.2 | 24.0 · 9.7 |
| MKV / bzip2 (Rust) (serial) | 8.1 · 5.2 | 17.4 · 3.5 |
| MKV / bzip2 (C) (serial) | 9.6 · 5.2 | 19.7 · 3.5 |
| ELF / bzip2-turbo | 13.5 · 7.6 | 36.7 · 5.4 |
| ELF / 7zz | 10.6 · 12.2 | 36.5 · 9.7 |
| ELF / bzip2 (Rust) (serial) | 12.1 · 5.2 | 24.8 · 3.5 |
| ELF / bzip2 (C) (serial) | 12.7 · 5.3 | 26.3 · 3.5 |

### Level 5, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 29.6 · 11.8 | 49.5 · 5.2 |
| JSON / 7zz | 21.3 · 18.3 | 50.4 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 15.0 · 5.3 | 32.8 · 3.5 |
| JSON / bzip2 (C) (serial) | 14.0 · 5.3 | 31.9 · 3.5 |
| Repeated text / bzip2-turbo | 8.4 · 11.2 | 339.0 · 5.1 |
| Repeated text / 7zz | 5.1 · 17.8 | 207.5 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 5.3 · 5.8 | 148.0 · 3.5 |
| Repeated text / bzip2 (C) (serial) | 4.6 · 5.6 | 132.8 · 3.5 |
| Logs / bzip2-turbo | 34.0 · 11.7 | 77.4 · 5.2 |
| Logs / 7zz | 23.2 · 18.2 | 79.9 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 13.1 · 5.2 | 51.7 · 3.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 5.2 | 47.6 · 3.5 |
| MKV / bzip2-turbo | 23.5 · 13.5 | 41.6 · 5.5 |
| MKV / 7zz | 18.5 · 19.7 | 41.8 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 8.1 · 5.3 | 17.4 · 3.5 |
| MKV / bzip2 (C) (serial) | 9.6 · 5.2 | 19.8 · 3.5 |
| ELF / bzip2-turbo | 23.9 · 13.5 | 54.5 · 5.5 |
| ELF / 7zz | 19.0 · 19.8 | 52.7 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 12.1 · 5.3 | 24.8 · 3.5 |
| ELF / bzip2 (C) (serial) | 12.8 · 5.3 | 26.3 · 3.5 |

### Level 5, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 54.5 · 21.5 | 49.5 · 5.1 |
| JSON / 7zz | 39.8 · 31.8 | 50.3 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 14.9 · 5.3 | 32.8 · 3.5 |
| JSON / bzip2 (C) (serial) | 14.0 · 5.3 | 31.5 · 3.5 |
| Repeated text / bzip2-turbo | 14.4 · 20.6 | 339.1 · 5.1 |
| Repeated text / 7zz | 8.9 · 30.8 | 208.0 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 5.3 · 5.8 | 148.1 · 3.5 |
| Repeated text / bzip2 (C) (serial) | 4.5 · 5.6 | 133.2 · 3.5 |
| Logs / bzip2-turbo | 63.8 · 21.0 | 75.8 · 5.1 |
| Logs / 7zz | 44.2 · 31.3 | 78.7 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 13.2 · 5.2 | 50.6 · 3.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 5.3 | 47.4 · 3.5 |
| MKV / bzip2-turbo | 43.4 · 24.9 | 41.6 · 5.5 |
| MKV / 7zz | 35.1 · 34.3 | 42.0 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 8.1 · 5.3 | 17.4 · 3.5 |
| MKV / bzip2 (C) (serial) | 9.6 · 5.3 | 19.6 · 3.5 |
| ELF / bzip2-turbo | 44.3 · 24.0 | 54.0 · 5.6 |
| ELF / 7zz | 36.0 · 33.8 | 52.7 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 12.1 · 5.3 | 24.5 · 3.5 |
| ELF / bzip2 (C) (serial) | 12.8 · 5.3 | 26.1 · 3.5 |

### Level 5, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 89.3 · 41.2 | 48.4 · 5.1 |
| JSON / 7zz | 70.5 · 58.8 | 49.4 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 14.9 · 5.3 | 31.8 · 3.5 |
| JSON / bzip2 (C) (serial) | 14.0 · 5.2 | 31.0 · 3.5 |
| Repeated text / bzip2-turbo | 24.3 · 39.1 | 338.4 · 5.1 |
| Repeated text / 7zz | 15.5 · 56.8 | 207.9 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 5.3 · 5.8 | 148.2 · 3.5 |
| Repeated text / bzip2 (C) (serial) | 4.5 · 5.6 | 133.1 · 3.5 |
| Logs / bzip2-turbo | 107.3 · 40.2 | 74.8 · 5.1 |
| Logs / 7zz | 81.4 · 57.8 | 79.9 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 13.2 · 5.3 | 51.6 · 3.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 5.2 | 47.8 · 3.5 |
| MKV / bzip2-turbo | 75.3 · 47.9 | 41.6 · 5.5 |
| MKV / 7zz | 64.0 · 63.9 | 41.9 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 8.1 · 5.2 | 17.0 · 3.5 |
| MKV / bzip2 (C) (serial) | 9.6 · 5.3 | 19.3 · 3.5 |
| ELF / bzip2-turbo | 69.1 · 43.9 | 54.2 · 5.5 |
| ELF / 7zz | 58.5 · 59.3 | 52.1 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 12.1 · 5.3 | 24.7 · 3.5 |
| ELF / bzip2 (C) (serial) | 12.8 · 5.3 | 26.1 · 3.5 |

### Level 9, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 13.6 · 11.1 | 20.9 · 7.8 |
| JSON / 7zz | 9.7 · 14.1 | 23.2 · 10.0 |
| JSON / bzip2 (Rust) (serial) | 13.5 · 7.8 | 19.6 · 5.2 |
| JSON / bzip2 (C) (serial) | 12.8 · 7.9 | 17.7 · 5.0 |
| Repeated text / bzip2-turbo | 4.1 · 10.4 | 335.7 · 7.8 |
| Repeated text / 7zz | 2.2 · 13.5 | 197.1 · 9.8 |
| Repeated text / bzip2 (Rust) (serial) | 4.9 · 8.7 | 146.8 · 5.0 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 8.8 | 132.1 · 5.0 |
| Logs / bzip2-turbo | 15.5 · 10.8 | 38.1 · 7.8 |
| Logs / 7zz | 10.6 · 13.9 | 36.0 · 10.0 |
| Logs / bzip2 (Rust) (serial) | 11.2 · 7.9 | 34.7 · 5.1 |
| Logs / bzip2 (C) (serial) | 9.9 · 7.9 | 31.2 · 5.0 |
| MKV / bzip2-turbo | 11.3 · 12.3 | 12.4 · 8.0 |
| MKV / 7zz | 8.7 · 15.4 | 13.3 · 10.0 |
| MKV / bzip2 (Rust) (serial) | 7.6 · 7.8 | 11.5 · 5.1 |
| MKV / bzip2 (C) (serial) | 9.0 · 7.9 | 13.0 · 5.1 |
| ELF / bzip2-turbo | 12.5 · 12.3 | 23.5 · 7.9 |
| ELF / 7zz | 9.9 · 15.3 | 22.9 · 10.0 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 7.8 | 18.2 · 5.1 |
| ELF / bzip2 (C) (serial) | 12.1 · 7.9 | 18.7 · 5.1 |

### Level 9, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 25.3 · 20.0 | 23.9 · 7.9 |
| JSON / 7zz | 18.4 · 23.5 | 23.6 · 10.1 |
| JSON / bzip2 (Rust) (serial) | 13.5 · 7.9 | 17.9 · 5.1 |
| JSON / bzip2 (C) (serial) | 12.8 · 7.8 | 16.6 · 5.0 |
| Repeated text / bzip2-turbo | 5.1 · 19.2 | 336.2 · 7.9 |
| Repeated text / 7zz | 3.3 · 22.5 | 205.1 · 10.0 |
| Repeated text / bzip2 (Rust) (serial) | 4.9 · 8.8 | 146.6 · 5.0 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 8.8 | 132.0 · 5.0 |
| Logs / bzip2-turbo | 29.7 · 19.6 | 40.0 · 7.9 |
| Logs / 7zz | 20.8 · 23.2 | 40.7 · 10.1 |
| Logs / bzip2 (Rust) (serial) | 11.3 · 7.8 | 30.3 · 5.1 |
| Logs / bzip2 (C) (serial) | 10.1 · 7.9 | 28.0 · 5.0 |
| MKV / bzip2-turbo | 22.1 · 22.8 | 23.1 · 8.5 |
| MKV / 7zz | 16.9 · 26.2 | 27.1 · 10.1 |
| MKV / bzip2 (Rust) (serial) | 7.7 · 7.9 | 11.4 · 5.1 |
| MKV / bzip2 (C) (serial) | 9.0 · 7.9 | 12.5 · 5.0 |
| ELF / bzip2-turbo | 21.9 · 22.1 | 29.1 · 8.6 |
| ELF / 7zz | 17.8 · 25.5 | 29.3 · 10.1 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 7.9 | 16.7 · 5.1 |
| ELF / bzip2 (C) (serial) | 12.1 · 7.8 | 18.0 · 5.1 |

### Level 9, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 46.0 · 38.0 | 21.2 · 7.8 |
| JSON / 7zz | 33.8 · 41.9 | 23.2 · 10.1 |
| JSON / bzip2 (Rust) (serial) | 13.4 · 7.9 | 17.5 · 5.1 |
| JSON / bzip2 (C) (serial) | 12.7 · 7.8 | 18.1 · 5.0 |
| Repeated text / bzip2-turbo | 8.5 · 36.0 | 336.1 · 7.8 |
| Repeated text / 7zz | 5.5 · 40.2 | 203.5 · 10.0 |
| Repeated text / bzip2 (Rust) (serial) | 4.8 · 8.7 | 146.5 · 5.0 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 8.8 | 131.6 · 5.0 |
| Logs / bzip2-turbo | 51.6 · 37.1 | 36.6 · 7.9 |
| Logs / 7zz | 37.1 · 41.4 | 39.1 · 10.1 |
| Logs / bzip2 (Rust) (serial) | 11.3 · 7.9 | 31.0 · 5.1 |
| Logs / bzip2 (C) (serial) | 10.0 · 7.8 | 27.1 · 5.0 |
| MKV / bzip2-turbo | 37.7 · 43.5 | 22.3 · 8.4 |
| MKV / 7zz | 30.4 · 47.4 | 18.9 · 10.1 |
| MKV / bzip2 (Rust) (serial) | 7.6 · 7.9 | 11.0 · 5.2 |
| MKV / bzip2 (C) (serial) | 9.0 · 7.9 | 11.4 · 5.1 |
| ELF / bzip2-turbo | 35.8 · 41.4 | 33.6 · 8.7 |
| ELF / 7zz | 32.6 · 45.0 | 31.7 · 10.1 |
| ELF / bzip2 (Rust) (serial) | 11.4 · 7.9 | 18.6 · 5.1 |
| ELF / bzip2 (C) (serial) | 12.0 · 7.8 | 19.5 · 5.1 |

### Level 9, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 66.7 · 74.1 | 22.1 · 7.8 |
| JSON / 7zz | 54.1 · 79.0 | 22.1 · 10.1 |
| JSON / bzip2 (Rust) (serial) | 13.4 · 7.8 | 14.3 · 5.2 |
| JSON / bzip2 (C) (serial) | 12.7 · 7.9 | 17.2 · 5.0 |
| Repeated text / bzip2-turbo | 11.9 · 70.1 | 334.9 · 7.9 |
| Repeated text / 7zz | 7.9 · 75.8 | 205.1 · 10.0 |
| Repeated text / bzip2 (Rust) (serial) | 4.8 · 8.8 | 146.5 · 5.0 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 8.7 | 132.1 · 5.0 |
| Logs / bzip2-turbo | 74.3 · 72.3 | 41.9 · 7.8 |
| Logs / 7zz | 58.4 · 77.9 | 43.8 · 10.1 |
| Logs / bzip2 (Rust) (serial) | 11.2 · 7.9 | 32.3 · 5.1 |
| Logs / bzip2 (C) (serial) | 10.0 · 7.9 | 32.3 · 5.0 |
| MKV / bzip2-turbo | 58.4 · 85.1 | 21.8 · 8.3 |
| MKV / 7zz | 49.4 · 89.9 | 22.8 · 10.1 |
| MKV / bzip2 (Rust) (serial) | 7.7 · 7.9 | 10.5 · 5.1 |
| MKV / bzip2 (C) (serial) | 9.0 · 7.9 | 12.0 · 5.1 |
| ELF / bzip2-turbo | 39.7 · 43.5 | 33.4 · 8.7 |
| ELF / 7zz | 33.2 · 47.3 | 35.1 · 10.1 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 7.8 | 19.2 · 5.1 |
| ELF / bzip2 (C) (serial) | 12.1 · 7.9 | 21.0 · 5.1 |

## Intel x86-64

Host: c7i.2xlarge, Intel Xeon Platinum 8488C, 16 GiB RAM, Ubuntu 24.04.

The host exposes 8 allowed logical CPUs across 4 physical cores. Every contender uses the same affinity set at a given requested budget; physical cores are selected before SMT. Native decoder capacity is 1/2/2/2 total threads at requested budgets 1/2/4/8; the 7zz setting is a request, not a measured live-thread count. The 4T and 8T decode rows below are prior results that measured the same two-thread native pipeline as 2T.

| Requested budget | Allowed logical CPUs |
|---|---|
| 1T | `[2]` |
| 2T | `[2 3]` |
| 4T | `[2 3 4 5]` |
| 8T | `[2 3 4 5 6 7 0 1]` |

| Engine | Executable SHA-256 |
|---|---|
| turbo | `0a844785f831232889ca515462ec8cac3bd39fdb509d43009a68de9c7bd0fed8` |
| 7zz | `4d7d10d75af4e060adb57c20e40d24317914321c4d0e916f49897642383b61a2` |
| pure | `d967d4035a69cfbc7c4fdb5ffbe7637b6f6ee8ba979acd1475f5992db69ee753` |
| c | `27c898b2c03e82face00e352d356e0b5ed5c58c87f7010ac2fe2eeb0f865aced` |

### Level 1, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 20.2 · 3.6 | 93.9 · 3.8 |
| JSON / 7zz | 13.6 · 7.6 | 94.5 · 10.4 |
| JSON / bzip2 (Rust) (serial) | 17.8 · 3.0 | 59.8 · 2.4 |
| JSON / bzip2 (C) (serial) | 16.9 · 2.9 | 55.4 · 2.4 |
| Repeated text / bzip2-turbo | 10.4 · 3.6 | 380.8 · 3.9 |
| Repeated text / 7zz | 3.6 · 7.5 | 253.8 · 10.4 |
| Repeated text / bzip2 (Rust) (serial) | 8.4 · 3.0 | 197.3 · 2.4 |
| Repeated text / bzip2 (C) (serial) | 6.6 · 3.1 | 178.6 · 2.4 |
| Logs / bzip2-turbo | 23.8 · 3.7 | 115.4 · 4.0 |
| Logs / 7zz | 14.8 · 7.6 | 117.8 · 10.3 |
| Logs / bzip2 (Rust) (serial) | 16.9 · 3.0 | 78.2 · 2.4 |
| Logs / bzip2 (C) (serial) | 15.9 · 2.9 | 71.0 · 2.4 |
| MKV / bzip2-turbo | 14.5 · 3.9 | 51.4 · 3.8 |
| MKV / 7zz | 12.1 · 7.7 | 36.2 · 10.4 |
| MKV / bzip2 (Rust) (serial) | 11.9 · 3.0 | 21.2 · 2.4 |
| MKV / bzip2 (C) (serial) | 11.1 · 3.0 | 23.9 · 2.4 |
| ELF / bzip2-turbo | 15.6 · 3.9 | 60.9 · 3.9 |
| ELF / 7zz | 12.6 · 7.7 | 50.7 · 10.5 |
| ELF / bzip2 (Rust) (serial) | 14.9 · 3.0 | 28.7 · 2.4 |
| ELF / bzip2 (C) (serial) | 14.1 · 3.1 | 28.6 · 2.5 |

### Level 1, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 30.7 · 5.3 | 118.2 · 3.4 |
| JSON / 7zz | 26.8 · 10.0 | 128.6 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 17.8 · 3.0 | 59.9 · 2.4 |
| JSON / bzip2 (C) (serial) | 16.8 · 2.9 | 55.4 · 2.4 |
| Repeated text / bzip2-turbo | 13.9 · 5.2 | 273.0 · 3.2 |
| Repeated text / 7zz | 7.0 · 9.7 | 254.0 · 10.5 |
| Repeated text / bzip2 (Rust) (serial) | 8.4 · 3.0 | 195.5 · 2.4 |
| Repeated text / bzip2 (C) (serial) | 6.6 · 3.1 | 177.8 · 2.4 |
| Logs / bzip2-turbo | 35.0 · 5.2 | 141.6 · 3.5 |
| Logs / 7zz | 28.9 · 9.9 | 159.3 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 16.9 · 3.0 | 78.0 · 2.4 |
| Logs / bzip2 (C) (serial) | 15.9 · 2.9 | 70.9 · 2.4 |
| MKV / bzip2-turbo | 21.6 · 5.5 | 72.8 · 3.3 |
| MKV / 7zz | 23.8 · 10.1 | 37.2 · 10.5 |
| MKV / bzip2 (Rust) (serial) | 11.9 · 3.1 | 21.1 · 2.4 |
| MKV / bzip2 (C) (serial) | 11.1 · 3.0 | 23.8 · 2.5 |
| ELF / bzip2-turbo | 24.4 · 5.7 | 84.0 · 3.8 |
| ELF / 7zz | 24.3 · 10.2 | 55.2 · 10.6 |
| ELF / bzip2 (Rust) (serial) | 14.8 · 3.1 | 28.5 · 2.4 |
| ELF / bzip2 (C) (serial) | 14.0 · 3.1 | 28.4 · 2.4 |

### Level 1, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 54.6 · 7.7 | 117.4 · 3.4 |
| JSON / 7zz | 51.6 · 14.1 | 120.7 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 17.8 · 3.1 | 59.8 · 2.4 |
| JSON / bzip2 (C) (serial) | 16.8 · 2.9 | 55.4 · 2.5 |
| Repeated text / bzip2-turbo | 28.1 · 7.5 | 264.0 · 3.3 |
| Repeated text / 7zz | 13.7 · 14.0 | 250.6 · 10.5 |
| Repeated text / bzip2 (Rust) (serial) | 8.4 · 3.0 | 193.3 · 2.4 |
| Repeated text / bzip2 (C) (serial) | 6.6 · 3.1 | 176.4 · 2.4 |
| Logs / bzip2-turbo | 71.0 · 7.7 | 132.9 · 3.5 |
| Logs / 7zz | 55.5 · 14.1 | 160.5 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 16.9 · 3.0 | 77.8 · 2.4 |
| Logs / bzip2 (C) (serial) | 15.9 · 3.0 | 70.7 · 2.4 |
| MKV / bzip2-turbo | 46.2 · 8.1 | 72.5 · 3.3 |
| MKV / 7zz | 45.5 · 14.7 | 37.0 · 10.5 |
| MKV / bzip2 (Rust) (serial) | 11.9 · 3.1 | 21.0 · 2.4 |
| MKV / bzip2 (C) (serial) | 11.1 · 3.0 | 23.8 · 2.4 |
| ELF / bzip2-turbo | 48.1 · 8.5 | 82.9 · 3.6 |
| ELF / 7zz | 44.3 · 14.8 | 52.6 · 10.4 |
| ELF / bzip2 (Rust) (serial) | 14.8 · 3.0 | 28.5 · 2.4 |
| ELF / bzip2 (C) (serial) | 14.0 · 3.0 | 28.3 · 2.4 |

### Level 1, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 81.3 · 12.6 | 114.4 · 3.4 |
| JSON / 7zz | 74.4 · 22.4 | 121.9 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 17.8 · 3.1 | 59.8 · 2.5 |
| JSON / bzip2 (C) (serial) | 16.8 · 2.9 | 55.4 · 2.5 |
| Repeated text / bzip2-turbo | 38.0 · 12.7 | 258.2 · 3.2 |
| Repeated text / 7zz | 24.4 · 22.5 | 249.9 · 10.5 |
| Repeated text / bzip2 (Rust) (serial) | 8.4 · 3.0 | 195.0 · 2.4 |
| Repeated text / bzip2 (C) (serial) | 6.6 · 3.1 | 176.8 · 2.4 |
| Logs / bzip2-turbo | 94.0 · 12.7 | 133.1 · 3.5 |
| Logs / 7zz | 81.5 · 22.5 | 157.6 · 10.6 |
| Logs / bzip2 (Rust) (serial) | 16.9 · 3.0 | 77.8 · 2.4 |
| Logs / bzip2 (C) (serial) | 15.9 · 2.9 | 70.6 · 2.4 |
| MKV / bzip2-turbo | 58.2 · 13.8 | 72.5 · 3.2 |
| MKV / 7zz | 55.3 · 23.5 | 37.1 · 10.6 |
| MKV / bzip2 (Rust) (serial) | 11.9 · 3.0 | 21.1 · 2.4 |
| MKV / bzip2 (C) (serial) | 11.1 · 3.1 | 23.8 · 2.5 |
| ELF / bzip2-turbo | 62.9 · 14.0 | 83.7 · 3.6 |
| ELF / 7zz | 57.0 · 23.8 | 52.5 · 10.6 |
| ELF / bzip2 (Rust) (serial) | 14.8 · 3.0 | 28.6 · 2.4 |
| ELF / bzip2 (C) (serial) | 14.0 · 3.0 | 28.4 · 2.5 |

### Level 5, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 16.7 · 7.4 | 83.0 · 5.9 |
| JSON / 7zz | 11.6 · 12.2 | 85.3 · 10.4 |
| JSON / bzip2 (Rust) (serial) | 15.8 · 5.7 | 49.0 · 3.9 |
| JSON / bzip2 (C) (serial) | 15.3 · 5.7 | 46.1 · 3.9 |
| Repeated text / bzip2-turbo | 8.0 · 7.0 | 376.4 · 6.0 |
| Repeated text / 7zz | 5.6 · 11.9 | 268.9 · 10.3 |
| Repeated text / bzip2 (Rust) (serial) | 6.4 · 6.2 | 176.9 · 3.9 |
| Repeated text / bzip2 (C) (serial) | 5.4 · 6.2 | 163.5 · 3.9 |
| Logs / bzip2-turbo | 19.7 · 7.4 | 107.8 · 6.0 |
| Logs / 7zz | 12.3 · 12.1 | 114.6 · 10.4 |
| Logs / bzip2 (Rust) (serial) | 14.5 · 5.6 | 70.5 · 3.9 |
| Logs / bzip2 (C) (serial) | 13.2 · 5.7 | 65.2 · 3.9 |
| MKV / bzip2-turbo | 13.6 · 8.0 | 45.3 · 5.9 |
| MKV / 7zz | 11.5 · 12.9 | 32.9 · 10.4 |
| MKV / bzip2 (Rust) (serial) | 12.0 · 5.7 | 19.0 · 3.9 |
| MKV / bzip2 (C) (serial) | 11.3 · 5.7 | 20.7 · 4.0 |
| ELF / bzip2-turbo | 14.6 · 8.0 | 55.2 · 5.9 |
| ELF / 7zz | 11.7 · 12.9 | 46.0 · 10.5 |
| ELF / bzip2 (Rust) (serial) | 15.4 · 5.7 | 26.8 · 3.9 |
| ELF / bzip2 (C) (serial) | 14.6 · 5.6 | 26.7 · 4.0 |

### Level 5, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 30.4 · 12.5 | 111.1 · 5.7 |
| JSON / 7zz | 22.5 · 19.0 | 119.1 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 15.8 · 5.6 | 49.2 · 3.9 |
| JSON / bzip2 (C) (serial) | 15.3 · 5.7 | 46.4 · 3.9 |
| Repeated text / bzip2-turbo | 12.2 · 11.9 | 345.2 · 5.7 |
| Repeated text / 7zz | 9.2 · 18.3 | 274.7 · 10.4 |
| Repeated text / bzip2 (Rust) (serial) | 6.4 · 6.2 | 175.6 · 3.9 |
| Repeated text / bzip2 (C) (serial) | 5.3 · 6.2 | 162.3 · 3.9 |
| Logs / bzip2-turbo | 37.0 · 12.1 | 130.6 · 5.6 |
| Logs / 7zz | 23.6 · 18.7 | 148.0 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 14.4 · 5.7 | 69.8 · 3.9 |
| Logs / bzip2 (C) (serial) | 13.2 · 5.7 | 64.3 · 4.0 |
| MKV / bzip2-turbo | 25.6 · 14.1 | 72.9 · 5.9 |
| MKV / 7zz | 22.2 · 20.4 | 40.7 · 10.4 |
| MKV / bzip2 (Rust) (serial) | 11.9 · 5.6 | 19.2 · 3.9 |
| MKV / bzip2 (C) (serial) | 11.2 · 5.6 | 20.9 · 3.9 |
| ELF / bzip2-turbo | 25.8 · 14.1 | 81.0 · 6.0 |
| ELF / 7zz | 20.9 · 20.5 | 58.1 · 10.5 |
| ELF / bzip2 (Rust) (serial) | 15.3 · 5.7 | 26.6 · 3.9 |
| ELF / bzip2 (C) (serial) | 14.6 · 5.7 | 26.8 · 3.9 |

### Level 5, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 58.7 · 22.3 | 110.1 · 5.7 |
| JSON / 7zz | 41.6 · 32.4 | 115.4 · 10.6 |
| JSON / bzip2 (Rust) (serial) | 15.8 · 5.7 | 48.7 · 3.9 |
| JSON / bzip2 (C) (serial) | 15.3 · 5.7 | 45.9 · 3.9 |
| Repeated text / bzip2-turbo | 21.1 · 21.1 | 329.8 · 5.8 |
| Repeated text / 7zz | 15.7 · 31.5 | 270.2 · 10.4 |
| Repeated text / bzip2 (Rust) (serial) | 6.4 · 6.2 | 174.8 · 3.9 |
| Repeated text / bzip2 (C) (serial) | 5.3 · 6.2 | 161.3 · 4.0 |
| Logs / bzip2-turbo | 65.9 · 21.7 | 129.7 · 5.7 |
| Logs / 7zz | 44.4 · 32.1 | 145.2 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 14.5 · 5.6 | 69.9 · 3.9 |
| Logs / bzip2 (C) (serial) | 13.2 · 5.6 | 64.7 · 4.0 |
| MKV / bzip2-turbo | 46.6 · 25.6 | 66.0 · 6.0 |
| MKV / 7zz | 41.7 · 35.2 | 40.6 · 10.5 |
| MKV / bzip2 (Rust) (serial) | 11.9 · 5.6 | 19.0 · 3.9 |
| MKV / bzip2 (C) (serial) | 11.2 · 5.7 | 20.9 · 3.9 |
| ELF / bzip2-turbo | 47.2 · 24.7 | 80.1 · 6.0 |
| ELF / 7zz | 39.2 · 34.5 | 57.6 · 10.6 |
| ELF / bzip2 (Rust) (serial) | 15.3 · 5.7 | 26.6 · 3.9 |
| ELF / bzip2 (C) (serial) | 14.6 · 5.7 | 26.6 · 3.9 |

### Level 5, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 67.1 · 41.9 | 110.3 · 5.7 |
| JSON / 7zz | 58.7 · 59.4 | 116.8 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 15.8 · 5.7 | 49.3 · 3.9 |
| JSON / bzip2 (C) (serial) | 15.2 · 5.7 | 47.1 · 3.8 |
| Repeated text / bzip2-turbo | 23.6 · 39.8 | 331.0 · 5.8 |
| Repeated text / 7zz | 18.7 · 57.4 | 266.7 · 10.4 |
| Repeated text / bzip2 (Rust) (serial) | 6.3 · 6.2 | 174.7 · 3.9 |
| Repeated text / bzip2 (C) (serial) | 5.3 · 6.1 | 161.2 · 3.9 |
| Logs / bzip2-turbo | 80.2 · 40.9 | 128.7 · 5.7 |
| Logs / 7zz | 65.6 · 58.5 | 146.0 · 10.6 |
| Logs / bzip2 (Rust) (serial) | 14.4 · 5.7 | 69.4 · 3.9 |
| Logs / bzip2 (C) (serial) | 13.2 · 5.6 | 64.2 · 3.9 |
| MKV / bzip2-turbo | 54.9 · 48.5 | 73.3 · 6.0 |
| MKV / 7zz | 49.3 · 64.5 | 40.6 · 10.6 |
| MKV / bzip2 (Rust) (serial) | 11.9 · 5.7 | 19.3 · 3.9 |
| MKV / bzip2 (C) (serial) | 11.2 · 5.6 | 21.0 · 3.9 |
| ELF / bzip2-turbo | 55.0 · 44.5 | 79.0 · 6.0 |
| ELF / 7zz | 46.1 · 59.9 | 57.7 · 10.5 |
| ELF / bzip2 (Rust) (serial) | 15.3 · 5.7 | 26.7 · 3.9 |
| ELF / bzip2 (C) (serial) | 14.5 · 5.7 | 26.7 · 3.9 |

### Level 9, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 15.2 · 11.6 | 41.9 · 8.2 |
| JSON / 7zz | 10.6 · 14.7 | 42.9 · 10.6 |
| JSON / bzip2 (Rust) (serial) | 14.6 · 8.3 | 32.9 · 5.6 |
| JSON / bzip2 (C) (serial) | 14.2 · 8.3 | 31.8 · 5.5 |
| Repeated text / bzip2-turbo | 5.0 · 10.9 | 313.2 · 8.2 |
| Repeated text / 7zz | 4.2 · 14.1 | 237.9 · 10.5 |
| Repeated text / bzip2 (Rust) (serial) | 4.8 · 9.1 | 182.0 · 5.5 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 9.2 | 170.6 · 5.5 |
| Logs / bzip2-turbo | 17.3 · 11.2 | 74.1 · 8.4 |
| Logs / 7zz | 11.3 · 14.6 | 78.4 · 10.6 |
| Logs / bzip2 (Rust) (serial) | 12.9 · 8.2 | 54.3 · 5.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 8.3 | 50.7 · 5.6 |
| MKV / bzip2-turbo | 13.2 · 12.9 | 29.2 · 8.3 |
| MKV / 7zz | 10.4 · 16.0 | 23.5 · 10.6 |
| MKV / bzip2 (Rust) (serial) | 10.9 · 8.3 | 15.7 · 5.5 |
| MKV / bzip2 (C) (serial) | 10.5 · 8.3 | 16.8 · 5.6 |
| ELF / bzip2-turbo | 13.9 · 12.9 | 43.1 · 8.3 |
| ELF / 7zz | 11.1 · 16.0 | 37.2 · 10.6 |
| ELF / bzip2 (Rust) (serial) | 14.6 · 8.4 | 23.5 · 5.5 |
| ELF / bzip2 (C) (serial) | 14.0 · 8.4 | 23.3 · 5.5 |

### Level 9, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 28.6 · 20.6 | 45.3 · 8.4 |
| JSON / 7zz | 20.3 · 24.1 | 49.9 · 10.8 |
| JSON / bzip2 (Rust) (serial) | 14.6 · 8.3 | 32.8 · 5.5 |
| JSON / bzip2 (C) (serial) | 14.2 · 8.3 | 31.7 · 5.5 |
| Repeated text / bzip2-turbo | 7.3 · 19.8 | 265.2 · 8.5 |
| Repeated text / 7zz | 7.5 · 23.2 | 235.1 · 10.7 |
| Repeated text / bzip2 (Rust) (serial) | 4.6 · 9.2 | 180.9 · 5.5 |
| Repeated text / bzip2 (C) (serial) | 4.1 · 9.1 | 168.4 · 5.4 |
| Logs / bzip2-turbo | 33.1 · 20.2 | 84.0 · 8.4 |
| Logs / 7zz | 22.1 · 23.8 | 88.9 · 10.8 |
| Logs / bzip2 (Rust) (serial) | 12.8 · 8.3 | 53.6 · 5.5 |
| Logs / bzip2 (C) (serial) | 11.6 · 8.3 | 49.5 · 5.5 |
| MKV / bzip2-turbo | 25.3 · 23.4 | 41.9 · 8.6 |
| MKV / 7zz | 20.3 · 26.8 | 40.0 · 10.9 |
| MKV / bzip2 (Rust) (serial) | 10.9 · 8.3 | 15.7 · 5.5 |
| MKV / bzip2 (C) (serial) | 10.4 · 8.3 | 16.6 · 5.5 |
| ELF / bzip2-turbo | 25.0 · 22.7 | 57.3 · 9.1 |
| ELF / 7zz | 20.6 · 26.2 | 47.4 · 10.9 |
| ELF / bzip2 (Rust) (serial) | 14.5 · 8.3 | 23.1 · 5.5 |
| ELF / bzip2 (C) (serial) | 13.9 · 8.4 | 23.1 · 5.6 |

### Level 9, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 47.9 · 38.7 | 46.3 · 8.4 |
| JSON / 7zz | 36.8 · 42.7 | 49.3 · 10.7 |
| JSON / bzip2 (Rust) (serial) | 14.3 · 8.3 | 31.5 · 5.6 |
| JSON / bzip2 (C) (serial) | 14.0 · 8.3 | 31.5 · 5.6 |
| Repeated text / bzip2-turbo | 14.4 · 36.8 | 274.7 · 8.4 |
| Repeated text / 7zz | 12.6 · 41.0 | 242.5 · 10.6 |
| Repeated text / bzip2 (Rust) (serial) | 4.8 · 9.2 | 178.7 · 5.5 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 9.1 | 167.2 · 5.5 |
| Logs / bzip2-turbo | 58.0 · 37.9 | 84.0 · 8.4 |
| Logs / 7zz | 38.9 · 41.9 | 87.6 · 10.7 |
| Logs / bzip2 (Rust) (serial) | 12.8 · 8.3 | 52.9 · 5.6 |
| Logs / bzip2 (C) (serial) | 11.6 · 8.2 | 49.7 · 5.5 |
| MKV / bzip2-turbo | 44.4 · 44.1 | 42.8 · 8.7 |
| MKV / 7zz | 36.3 · 48.2 | 39.6 · 10.8 |
| MKV / bzip2 (Rust) (serial) | 10.9 · 8.3 | 15.7 · 5.5 |
| MKV / bzip2 (C) (serial) | 10.5 · 8.3 | 16.5 · 5.6 |
| ELF / bzip2-turbo | 40.7 · 41.9 | 56.6 · 9.1 |
| ELF / 7zz | 36.2 · 45.7 | 46.0 · 10.7 |
| ELF / bzip2 (Rust) (serial) | 14.4 · 8.4 | 23.1 · 5.5 |
| ELF / bzip2 (C) (serial) | 13.8 · 8.4 | 22.9 · 5.6 |

### Level 9, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 59.0 · 74.6 | 45.6 · 8.4 |
| JSON / 7zz | 51.0 · 79.7 | 46.8 · 10.8 |
| JSON / bzip2 (Rust) (serial) | 14.6 · 8.3 | 32.1 · 5.5 |
| JSON / bzip2 (C) (serial) | 14.2 · 8.2 | 31.6 · 5.5 |
| Repeated text / bzip2-turbo | 12.9 · 70.9 | 286.0 · 8.5 |
| Repeated text / 7zz | 11.6 · 76.4 | 238.5 · 10.7 |
| Repeated text / bzip2 (Rust) (serial) | 4.8 · 9.1 | 180.7 · 5.6 |
| Repeated text / bzip2 (C) (serial) | 4.1 · 9.1 | 167.1 · 5.5 |
| Logs / bzip2-turbo | 64.2 · 73.1 | 85.0 · 8.4 |
| Logs / 7zz | 52.5 · 78.7 | 86.8 · 10.7 |
| Logs / bzip2 (Rust) (serial) | 12.8 · 8.3 | 52.5 · 5.5 |
| Logs / bzip2 (C) (serial) | 11.6 · 8.3 | 50.1 · 5.4 |
| MKV / bzip2-turbo | 49.8 · 85.7 | 42.9 · 8.6 |
| MKV / 7zz | 42.5 · 90.6 | 39.9 · 10.8 |
| MKV / bzip2 (Rust) (serial) | 10.9 · 8.2 | 15.5 · 5.5 |
| MKV / bzip2 (C) (serial) | 10.4 · 8.3 | 16.7 · 5.6 |
| ELF / bzip2-turbo | 41.1 · 44.2 | 55.1 · 9.0 |
| ELF / 7zz | 36.6 · 48.1 | 46.7 · 10.8 |
| ELF / bzip2 (Rust) (serial) | 14.4 · 8.4 | 23.3 · 5.5 |
| ELF / bzip2 (C) (serial) | 13.9 · 8.4 | 22.9 · 5.5 |

Raw per-round reports, canonical fixture receipts and build proofs remain outside Git. The build proofs anchor source, isolated backend graphs and executable identities; report self-consistency is not a substitute for those proofs. See [benchmark reproduction](../bench/bzip2-turbo-bench/README.md).

