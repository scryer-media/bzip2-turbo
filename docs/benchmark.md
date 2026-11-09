# Benchmark details

Measured 2026-11-08 on bzip2-turbo 0.2.0 (signed source `3225e57`) on AWS c7g.2xlarge and c7i.2xlarge. Each host contains 60 settings: five fixtures, levels 1/5/9 and requested budgets 1/2/4/8. All engines use `level × 100,000`-byte blocks. Native turbo and 7zz use one optimization pass; the serial Rust/C references do not expose that pass option. All four encoders were cross-decoded by all four decoders before timing; native turbo/7zz encoded bytes matched. Every timed decoder reads the same 7zz stream. Four balanced measured rounds follow one warmup. Rates use actual original input bytes and per-fixture median wall seconds; MiB is 1,048,576 bytes.

The output sink is `/dev/null` for every timed engine and operation. Wall time includes GNU time wrapper startup, codec startup and I/O. RSS is median peak codec-command RSS from GNU time's own child report (`gnu-time-child-v1`), not the Go parent's pre-exec high-water mark. CPU fields are reported at GNU time's hundredth-second precision. RSS is not aggregate simultaneous process-tree memory. Budgets are requests: native decoding uses at most two total threads; the pure Rust and C references remain serial at every comparison budget.

Build receipts record Rust 1.97.1 with optimization level 3, fat LTO, one codegen unit and stripped executables, with no target-CPU or extra Rust flag override. The Go harness uses Go 1.23.12. 7zz 26.04 uses pinned SDK source `9128b80e3a471108678e2b3ec3c985861c8d7b0f`, GCC 13.3.0 and its stock makefile.gcc build. Serial references use bzip2 0.6.1 with libbz2-rs-sys 0.2.6, or the bundled static libbz2 1.0.8 backend.

Signed source revision: `3225e57cadfc266765e8f46d851373091c87de53`. Source content SHA-256: `2e9dcc57a832574173328792b3db457cd06419cc97e1452421de666cc212cc6b`. Approved source archive SHA-256: `944435b21a8277ad132ff8af682e69e41985247d7b820fb71087abbbd33525a8`.

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

The host exposes 8 allowed logical CPUs across 8 physical cores. Every contender uses the same affinity set at a given requested budget; physical cores are selected before SMT. Native turbo runs as many decode workers as the requested budget; the 7zz setting is a request, not a measured live-thread count.

| Requested budget | Allowed logical CPUs |
|---|---|
| 1T | `[2]` |
| 2T | `[2 3]` |
| 4T | `[2 3 4 5]` |
| 8T | `[2 3 4 5 6 7 0 1]` |

| Engine | Executable SHA-256 |
|---|---|
| turbo | `ec91243ccd17bb1d514d88b6b8554cb08286c8742e7495e630c2939e2d668e1c` |
| 7zz | `765161e82b3e43a23a4fcd2492723a8154d640aa0b4abf5823787978b3368a1f` |
| pure | `1b827f8cbde07bcb328cca02d49144c02c659272fcd624c2c5dd1ce80dce794d` |
| c | `df9ac406e1ff49ddf95900534054b8ca6acbacb3ac1108c8344ffb9a42492ca1` |

### Level 1, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 23.3 · 2.9 | 96.1 · 7.0 |
| JSON / 7zz | 12.9 · 6.9 | 94.4 · 9.7 |
| JSON / bzip2 (Rust) (serial) | 16.3 · 2.6 | 58.5 · 2.0 |
| JSON / bzip2 (C) (serial) | 15.3 · 2.5 | 55.7 · 2.0 |
| Repeated text / bzip2-turbo | 42.0 · 2.8 | 347.4 · 7.0 |
| Repeated text / 7zz | 4.1 · 6.8 | 198.7 · 9.6 |
| Repeated text / bzip2 (Rust) (serial) | 6.9 · 2.7 | 149.5 · 2.0 |
| Repeated text / bzip2 (C) (serial) | 5.6 · 2.7 | 134.8 · 2.0 |
| Logs / bzip2-turbo | 27.8 · 2.8 | 118.5 · 7.0 |
| Logs / 7zz | 13.9 · 6.9 | 116.1 · 9.7 |
| Logs / bzip2 (Rust) (serial) | 15.2 · 2.6 | 77.3 · 2.0 |
| Logs / bzip2 (C) (serial) | 14.3 · 2.5 | 69.7 · 2.0 |
| MKV / bzip2-turbo | 11.8 · 3.1 | 35.6 · 7.0 |
| MKV / 7zz | 10.2 · 7.0 | 36.1 · 9.7 |
| MKV / bzip2 (Rust) (serial) | 7.8 · 2.6 | 23.3 · 2.0 |
| MKV / bzip2 (C) (serial) | 9.5 · 2.5 | 27.6 · 2.0 |
| ELF / bzip2-turbo | 15.1 · 3.1 | 50.5 · 7.0 |
| ELF / 7zz | 11.5 · 7.0 | 50.4 · 9.7 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 2.6 | 30.3 · 2.0 |
| ELF / bzip2 (C) (serial) | 12.3 · 2.5 | 32.8 · 2.0 |

### Level 1, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 45.6 · 4.7 | 169.3 · 15.7 |
| JSON / 7zz | 25.3 · 9.3 | 155.4 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 16.3 · 2.6 | 58.8 · 2.0 |
| JSON / bzip2 (C) (serial) | 15.3 · 2.5 | 55.8 · 2.0 |
| Repeated text / bzip2-turbo | 82.3 · 4.2 | 552.7 · 14.2 |
| Repeated text / 7zz | 8.1 · 9.0 | 209.0 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 6.9 · 2.6 | 149.3 · 2.0 |
| Repeated text / bzip2 (C) (serial) | 5.5 · 2.6 | 134.9 · 2.0 |
| Logs / bzip2-turbo | 55.0 · 4.5 | 207.7 · 14.9 |
| Logs / 7zz | 27.4 · 9.2 | 172.7 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 15.2 · 2.6 | 77.5 · 2.0 |
| Logs / bzip2 (C) (serial) | 14.3 · 2.5 | 69.3 · 2.0 |
| MKV / bzip2-turbo | 23.3 · 6.2 | 62.4 · 15.2 |
| MKV / 7zz | 20.2 · 9.5 | 42.6 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 7.8 · 2.6 | 23.3 · 2.0 |
| MKV / bzip2 (C) (serial) | 9.5 · 2.5 | 27.6 · 2.0 |
| ELF / bzip2-turbo | 29.2 · 6.0 | 83.4 · 14.3 |
| ELF / 7zz | 22.5 · 9.5 | 61.6 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 2.7 | 30.4 · 2.0 |
| ELF / bzip2 (C) (serial) | 12.3 · 2.5 | 32.8 · 2.0 |

### Level 1, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 90.1 · 7.4 | 308.0 · 28.8 |
| JSON / 7zz | 50.8 · 13.3 | 155.8 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 16.3 · 2.6 | 58.9 · 2.0 |
| JSON / bzip2 (C) (serial) | 15.3 · 2.5 | 55.5 · 2.0 |
| Repeated text / bzip2-turbo | 161.4 · 6.5 | 902.8 · 26.4 |
| Repeated text / 7zz | 16.1 · 13.3 | 209.4 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 7.0 · 2.7 | 149.7 · 2.0 |
| Repeated text / bzip2 (C) (serial) | 5.6 · 2.7 | 135.1 · 2.0 |
| Logs / bzip2-turbo | 107.6 · 7.0 | 370.4 · 28.0 |
| Logs / 7zz | 54.5 · 13.2 | 173.1 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 15.2 · 2.6 | 77.5 · 2.0 |
| Logs / bzip2 (C) (serial) | 14.3 · 2.5 | 69.8 · 2.0 |
| MKV / bzip2-turbo | 46.0 · 10.4 | 120.6 · 26.3 |
| MKV / 7zz | 40.3 · 13.9 | 42.6 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 7.8 · 2.6 | 23.3 · 2.0 |
| MKV / bzip2 (C) (serial) | 9.5 · 2.5 | 27.6 · 2.0 |
| ELF / bzip2-turbo | 55.9 · 9.4 | 138.6 · 25.5 |
| ELF / 7zz | 42.3 · 14.1 | 61.8 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 2.6 | 30.4 · 2.0 |
| ELF / bzip2 (C) (serial) | 12.3 · 2.5 | 32.9 · 2.0 |

### Level 1, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 170.8 · 12.8 | 492.8 · 51.6 |
| JSON / 7zz | 99.1 · 21.7 | 155.7 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 16.3 · 2.7 | 58.8 · 2.0 |
| JSON / bzip2 (C) (serial) | 15.3 · 2.5 | 55.6 · 2.0 |
| Repeated text / bzip2-turbo | 299.9 · 11.0 | 1168.3 · 50.6 |
| Repeated text / 7zz | 31.7 · 21.8 | 209.1 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 6.9 · 2.6 | 149.5 · 2.0 |
| Repeated text / bzip2 (C) (serial) | 5.6 · 2.6 | 135.2 · 2.0 |
| Logs / bzip2-turbo | 206.3 · 12.0 | 579.1 · 49.8 |
| Logs / 7zz | 104.3 · 21.7 | 173.3 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 15.2 · 2.6 | 77.5 · 2.0 |
| Logs / bzip2 (C) (serial) | 14.3 · 2.5 | 69.8 · 2.0 |
| MKV / bzip2-turbo | 89.0 · 18.9 | 205.9 · 52.6 |
| MKV / 7zz | 77.5 · 22.8 | 42.6 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 7.8 · 2.6 | 23.3 · 2.0 |
| MKV / bzip2 (C) (serial) | 9.5 · 2.5 | 27.7 · 2.0 |
| ELF / bzip2-turbo | 96.5 · 15.2 | 187.5 · 47.8 |
| ELF / 7zz | 76.5 · 23.2 | 61.8 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 2.7 | 30.4 · 2.0 |
| ELF / bzip2 (C) (serial) | 12.3 · 2.5 | 32.8 · 2.0 |

### Level 5, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 22.1 · 6.7 | 42.8 · 7.0 |
| JSON / 7zz | 10.8 · 11.5 | 42.3 · 9.7 |
| JSON / bzip2 (Rust) (serial) | 14.8 · 5.2 | 32.8 · 3.5 |
| JSON / bzip2 (C) (serial) | 13.9 · 5.3 | 32.0 · 3.5 |
| Repeated text / bzip2-turbo | 41.4 · 6.3 | 348.5 · 7.0 |
| Repeated text / 7zz | 3.1 · 11.2 | 198.8 · 9.6 |
| Repeated text / bzip2 (Rust) (serial) | 5.3 · 5.7 | 148.0 · 3.5 |
| Repeated text / bzip2 (C) (serial) | 4.6 · 5.6 | 133.1 · 3.5 |
| Logs / bzip2-turbo | 26.5 · 6.6 | 67.8 · 7.0 |
| Logs / 7zz | 11.9 · 11.4 | 66.1 · 9.7 |
| Logs / bzip2 (Rust) (serial) | 13.1 · 5.2 | 51.0 · 3.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 5.2 | 47.3 · 3.5 |
| MKV / bzip2-turbo | 10.9 · 7.5 | 23.8 · 7.0 |
| MKV / 7zz | 9.4 · 12.2 | 24.1 · 9.7 |
| MKV / bzip2 (Rust) (serial) | 8.0 · 5.3 | 17.4 · 3.5 |
| MKV / bzip2 (C) (serial) | 9.6 · 5.2 | 19.8 · 3.5 |
| ELF / bzip2-turbo | 14.4 · 7.4 | 36.9 · 7.0 |
| ELF / 7zz | 10.5 · 12.2 | 36.5 · 9.7 |
| ELF / bzip2 (Rust) (serial) | 12.0 · 5.3 | 24.7 · 3.5 |
| ELF / bzip2 (C) (serial) | 12.7 · 5.3 | 26.2 · 3.5 |

### Level 5, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 42.6 · 15.2 | 80.4 · 14.4 |
| JSON / 7zz | 21.1 · 18.3 | 50.4 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 14.8 · 5.2 | 32.7 · 3.5 |
| JSON / bzip2 (C) (serial) | 13.9 · 5.2 | 32.0 · 3.5 |
| Repeated text / bzip2-turbo | 80.9 · 13.8 | 606.6 · 13.6 |
| Repeated text / 7zz | 5.1 · 17.8 | 207.4 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 5.3 · 5.7 | 147.9 · 3.5 |
| Repeated text / bzip2 (C) (serial) | 4.6 · 5.6 | 133.6 · 3.5 |
| Logs / bzip2-turbo | 52.3 · 14.5 | 124.4 · 13.6 |
| Logs / 7zz | 23.2 · 18.2 | 78.4 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 13.1 · 5.3 | 51.0 · 3.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 5.2 | 47.4 · 3.5 |
| MKV / bzip2-turbo | 21.1 · 19.5 | 42.1 · 16.2 |
| MKV / 7zz | 18.2 · 19.8 | 42.4 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 8.0 · 5.3 | 17.3 · 3.5 |
| MKV / bzip2 (C) (serial) | 9.6 · 5.3 | 19.6 · 3.5 |
| ELF / bzip2-turbo | 27.0 · 17.8 | 57.1 · 14.4 |
| ELF / 7zz | 18.7 · 19.8 | 52.3 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 12.0 · 5.2 | 24.6 · 3.5 |
| ELF / bzip2 (C) (serial) | 12.7 · 5.2 | 26.2 · 3.5 |

### Level 5, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 80.4 · 27.9 | 150.2 · 24.6 |
| JSON / 7zz | 39.9 · 31.7 | 51.6 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 14.9 · 5.3 | 33.7 · 3.5 |
| JSON / bzip2 (C) (serial) | 14.0 · 5.2 | 33.0 · 3.5 |
| Repeated text / bzip2-turbo | 149.1 · 25.5 | 975.1 · 26.3 |
| Repeated text / 7zz | 9.1 · 30.7 | 207.8 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 5.3 · 5.7 | 148.3 · 3.5 |
| Repeated text / bzip2 (C) (serial) | 4.6 · 5.6 | 133.1 · 3.5 |
| Logs / bzip2-turbo | 97.9 · 27.0 | 228.4 · 23.9 |
| Logs / 7zz | 44.4 · 31.2 | 78.6 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 13.1 · 5.2 | 51.0 · 3.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 5.3 | 47.2 · 3.5 |
| MKV / bzip2-turbo | 40.0 · 37.1 | 79.7 · 28.4 |
| MKV / 7zz | 34.5 · 34.5 | 42.5 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 8.0 · 5.3 | 17.6 · 3.5 |
| MKV / bzip2 (C) (serial) | 9.6 · 5.2 | 19.7 · 3.5 |
| ELF / bzip2-turbo | 45.2 · 26.8 | 88.1 · 24.6 |
| ELF / 7zz | 35.5 · 33.8 | 52.8 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 12.1 · 5.2 | 24.6 · 3.5 |
| ELF / bzip2 (C) (serial) | 12.7 · 5.2 | 26.2 · 3.5 |

### Level 5, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 135.9 · 53.3 | 240.8 · 44.9 |
| JSON / 7zz | 70.4 · 58.8 | 51.8 · 9.8 |
| JSON / bzip2 (Rust) (serial) | 14.9 · 5.2 | 33.0 · 3.5 |
| JSON / bzip2 (C) (serial) | 13.9 · 5.3 | 32.1 · 3.5 |
| Repeated text / bzip2-turbo | 243.8 · 46.2 | 1263.9 · 49.6 |
| Repeated text / 7zz | 16.0 · 56.8 | 208.0 · 9.7 |
| Repeated text / bzip2 (Rust) (serial) | 5.4 · 5.8 | 148.2 · 3.5 |
| Repeated text / bzip2 (C) (serial) | 4.6 · 5.6 | 133.6 · 3.5 |
| Logs / bzip2-turbo | 168.8 · 49.5 | 378.1 · 44.1 |
| Logs / 7zz | 80.7 · 57.8 | 79.6 · 9.8 |
| Logs / bzip2 (Rust) (serial) | 13.1 · 5.2 | 51.4 · 3.5 |
| Logs / bzip2 (C) (serial) | 11.7 · 5.2 | 47.8 · 3.5 |
| MKV / bzip2-turbo | 71.1 · 72.4 | 133.5 · 52.7 |
| MKV / 7zz | 63.7 · 63.9 | 42.5 · 9.8 |
| MKV / bzip2 (Rust) (serial) | 8.1 · 5.2 | 17.6 · 3.5 |
| MKV / bzip2 (C) (serial) | 9.6 · 5.3 | 19.9 · 3.5 |
| ELF / bzip2-turbo | 64.2 · 43.2 | 120.4 · 44.8 |
| ELF / 7zz | 58.6 · 59.3 | 53.9 · 9.8 |
| ELF / bzip2 (Rust) (serial) | 12.0 · 5.2 | 24.9 · 3.5 |
| ELF / bzip2 (C) (serial) | 12.8 · 5.2 | 26.6 · 3.5 |

### Level 9, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 19.8 · 10.4 | 33.4 · 9.0 |
| JSON / 7zz | 10.0 · 14.1 | 33.4 · 10.0 |
| JSON / bzip2 (Rust) (serial) | 13.6 · 7.9 | 27.0 · 5.1 |
| JSON / bzip2 (C) (serial) | 12.9 · 7.9 | 26.8 · 5.0 |
| Repeated text / bzip2-turbo | 41.2 · 9.9 | 350.1 · 9.0 |
| Repeated text / 7zz | 2.3 · 13.6 | 198.1 · 9.8 |
| Repeated text / bzip2 (Rust) (serial) | 5.0 · 8.8 | 147.4 · 5.0 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 8.8 | 132.8 · 5.0 |
| Logs / bzip2-turbo | 23.8 · 10.2 | 45.2 · 9.0 |
| Logs / 7zz | 10.9 · 13.9 | 45.4 · 10.0 |
| Logs / bzip2 (Rust) (serial) | 11.4 · 7.9 | 38.7 · 5.1 |
| Logs / bzip2 (C) (serial) | 10.2 · 7.9 | 36.8 · 5.0 |
| MKV / bzip2-turbo | 9.9 · 11.7 | 15.9 · 9.0 |
| MKV / 7zz | 8.8 · 15.4 | 16.7 · 10.0 |
| MKV / bzip2 (Rust) (serial) | 7.6 · 7.9 | 13.9 · 5.2 |
| MKV / bzip2 (C) (serial) | 9.1 · 7.9 | 15.0 · 5.1 |
| ELF / bzip2-turbo | 13.3 · 11.7 | 26.4 · 9.0 |
| ELF / 7zz | 9.9 · 15.3 | 27.0 · 10.0 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 7.9 | 20.4 · 5.2 |
| ELF / bzip2 (C) (serial) | 12.2 · 7.9 | 21.6 · 5.1 |

### Level 9, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 38.0 · 25.0 | 61.4 · 18.4 |
| JSON / 7zz | 19.0 · 23.5 | 37.7 · 10.1 |
| JSON / bzip2 (Rust) (serial) | 13.6 · 7.9 | 26.1 · 5.0 |
| JSON / bzip2 (C) (serial) | 13.0 · 7.9 | 25.0 · 5.0 |
| Repeated text / bzip2-turbo | 78.7 · 23.0 | 619.4 · 17.1 |
| Repeated text / 7zz | 3.5 · 22.6 | 205.9 · 10.0 |
| Repeated text / bzip2 (Rust) (serial) | 5.0 · 8.8 | 147.7 · 5.0 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 8.7 | 133.5 · 5.0 |
| Logs / bzip2-turbo | 46.2 · 24.5 | 94.0 · 17.7 |
| Logs / 7zz | 21.2 · 23.2 | 57.4 · 10.1 |
| Logs / bzip2 (Rust) (serial) | 11.4 · 7.9 | 41.5 · 5.1 |
| Logs / bzip2 (C) (serial) | 10.2 · 7.9 | 39.4 · 5.0 |
| MKV / bzip2-turbo | 19.5 · 33.3 | 27.8 · 21.2 |
| MKV / 7zz | 17.3 · 26.2 | 34.0 · 10.1 |
| MKV / bzip2 (Rust) (serial) | 7.7 · 7.9 | 13.7 · 5.1 |
| MKV / bzip2 (C) (serial) | 9.0 · 7.9 | 15.4 · 5.1 |
| ELF / bzip2-turbo | 23.5 · 24.7 | 39.8 · 18.4 |
| ELF / 7zz | 18.6 · 25.4 | 36.5 · 10.1 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 7.8 | 20.4 · 5.1 |
| ELF / bzip2 (C) (serial) | 12.2 · 7.8 | 21.5 · 5.1 |

### Level 9, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 70.5 · 47.7 | 101.7 · 32.6 |
| JSON / 7zz | 36.5 · 41.9 | 37.9 · 10.1 |
| JSON / bzip2 (Rust) (serial) | 13.5 · 7.9 | 27.2 · 5.2 |
| JSON / bzip2 (C) (serial) | 13.0 · 7.9 | 27.1 · 5.0 |
| Repeated text / bzip2-turbo | 141.5 · 44.1 | 965.7 · 33.8 |
| Repeated text / 7zz | 5.8 · 40.4 | 206.4 · 10.0 |
| Repeated text / bzip2 (Rust) (serial) | 5.0 · 8.8 | 147.7 · 5.0 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 8.7 | 132.8 · 5.0 |
| Logs / bzip2-turbo | 82.7 · 46.2 | 145.9 · 31.8 |
| Logs / 7zz | 38.2 · 41.5 | 52.5 · 10.1 |
| Logs / bzip2 (Rust) (serial) | 11.6 · 7.9 | 38.6 · 5.1 |
| Logs / bzip2 (C) (serial) | 10.3 · 7.9 | 36.6 · 5.0 |
| MKV / bzip2-turbo | 35.1 · 64.7 | 48.6 · 39.5 |
| MKV / 7zz | 31.2 · 47.4 | 32.7 · 10.1 |
| MKV / bzip2 (Rust) (serial) | 7.7 · 7.8 | 13.0 · 5.1 |
| MKV / bzip2 (C) (serial) | 9.1 · 7.9 | 14.8 · 5.2 |
| ELF / bzip2-turbo | 36.8 · 41.1 | 58.7 · 33.5 |
| ELF / 7zz | 34.1 · 45.0 | 37.6 · 10.1 |
| ELF / bzip2 (Rust) (serial) | 11.5 · 7.8 | 20.2 · 5.1 |
| ELF / bzip2 (C) (serial) | 12.2 · 7.8 | 22.4 · 5.2 |

### Level 9, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 110.7 · 80.4 | 129.2 · 62.0 |
| JSON / 7zz | 57.3 · 78.9 | 38.8 · 10.1 |
| JSON / bzip2 (Rust) (serial) | 13.6 · 7.9 | 27.3 · 5.1 |
| JSON / bzip2 (C) (serial) | 12.9 · 7.9 | 27.2 · 5.0 |
| Repeated text / bzip2-turbo | 211.3 · 74.8 | 1211.0 · 63.1 |
| Repeated text / 7zz | 10.0 · 75.8 | 207.1 · 9.9 |
| Repeated text / bzip2 (Rust) (serial) | 5.0 · 8.7 | 148.1 · 5.0 |
| Repeated text / bzip2 (C) (serial) | 4.2 · 8.8 | 132.8 · 5.0 |
| Logs / bzip2-turbo | 129.0 · 77.8 | 201.7 · 60.1 |
| Logs / 7zz | 60.5 · 78.0 | 51.8 · 10.1 |
| Logs / bzip2 (Rust) (serial) | 11.5 · 7.9 | 38.9 · 5.1 |
| Logs / bzip2 (C) (serial) | 10.3 · 7.9 | 36.4 · 5.0 |
| MKV / bzip2-turbo | 55.7 · 104.4 | 74.0 · 73.9 |
| MKV / 7zz | 50.5 · 90.0 | 35.5 · 10.1 |
| MKV / bzip2 (Rust) (serial) | 7.7 · 7.8 | 13.9 · 5.2 |
| MKV / bzip2 (C) (serial) | 9.1 · 7.9 | 15.7 · 5.1 |
| ELF / bzip2-turbo | 37.1 · 42.8 | 73.9 · 37.6 |
| ELF / 7zz | 34.6 · 47.3 | 43.5 · 10.1 |
| ELF / bzip2 (Rust) (serial) | 11.6 · 7.8 | 22.4 · 5.2 |
| ELF / bzip2 (C) (serial) | 12.3 · 7.9 | 23.9 · 5.1 |

## Intel x86-64

Host: c7i.2xlarge, Intel Xeon Platinum 8488C, 16 GiB RAM, Ubuntu 24.04.

The host exposes 8 allowed logical CPUs across 4 physical cores. Every contender uses the same affinity set at a given requested budget; physical cores are selected before SMT. Native turbo runs as many decode workers as the requested budget; the 7zz setting is a request, not a measured live-thread count.

| Requested budget | Allowed logical CPUs |
|---|---|
| 1T | `[2]` |
| 2T | `[2 3]` |
| 4T | `[2 3 4 5]` |
| 8T | `[2 3 4 5 6 7 0 1]` |

| Engine | Executable SHA-256 |
|---|---|
| turbo | `ec8fa9c00151890b04c386451119bc76629d8a8dd67765a2da406b8c8849e1ab` |
| 7zz | `4d7d10d75af4e060adb57c20e40d24317914321c4d0e916f49897642383b61a2` |
| pure | `d967d4035a69cfbc7c4fdb5ffbe7637b6f6ee8ba979acd1475f5992db69ee753` |
| c | `27c898b2c03e82face00e352d356e0b5ed5c58c87f7010ac2fe2eeb0f865aced` |

### Level 1, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 24.5 · 3.5 | 86.0 · 7.6 |
| JSON / 7zz | 13.1 · 7.6 | 87.6 · 10.4 |
| JSON / bzip2 (Rust) (serial) | 16.9 · 3.1 | 55.6 · 2.4 |
| JSON / bzip2 (C) (serial) | 16.3 · 3.0 | 51.8 · 2.3 |
| Repeated text / bzip2-turbo | 46.3 · 3.3 | 362.4 · 7.5 |
| Repeated text / 7zz | 3.3 · 7.5 | 233.8 · 10.2 |
| Repeated text / bzip2 (Rust) (serial) | 7.8 · 3.0 | 182.5 · 2.4 |
| Repeated text / bzip2 (C) (serial) | 6.1 · 3.0 | 165.5 · 2.4 |
| Logs / bzip2-turbo | 30.8 · 3.4 | 107.4 · 7.5 |
| Logs / 7zz | 14.3 · 7.7 | 109.5 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 16.1 · 3.0 | 72.3 · 2.4 |
| Logs / bzip2 (C) (serial) | 15.4 · 2.9 | 65.8 · 2.4 |
| MKV / bzip2-turbo | 12.4 · 3.6 | 47.1 · 7.6 |
| MKV / 7zz | 11.3 · 7.6 | 33.6 · 10.4 |
| MKV / bzip2 (Rust) (serial) | 11.0 · 3.0 | 19.7 · 2.4 |
| MKV / bzip2 (C) (serial) | 10.3 · 3.1 | 22.2 · 2.4 |
| ELF / bzip2-turbo | 15.5 · 3.6 | 54.5 · 7.6 |
| ELF / 7zz | 11.7 · 7.7 | 47.0 · 10.4 |
| ELF / bzip2 (Rust) (serial) | 13.8 · 3.1 | 26.7 · 2.4 |
| ELF / bzip2 (C) (serial) | 13.0 · 3.0 | 26.5 · 2.4 |

### Level 1, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 46.8 · 5.2 | 153.2 · 16.3 |
| JSON / 7zz | 25.0 · 9.9 | 116.6 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 16.5 · 3.0 | 55.5 · 2.4 |
| JSON / bzip2 (C) (serial) | 15.6 · 3.0 | 51.6 · 2.5 |
| Repeated text / bzip2-turbo | 89.8 · 4.9 | 559.5 · 14.7 |
| Repeated text / 7zz | 6.6 · 9.8 | 238.4 · 10.5 |
| Repeated text / bzip2 (Rust) (serial) | 7.8 · 3.0 | 180.9 · 2.4 |
| Repeated text / bzip2 (C) (serial) | 6.1 · 3.1 | 164.6 · 2.5 |
| Logs / bzip2-turbo | 58.5 · 5.1 | 188.9 · 15.6 |
| Logs / 7zz | 27.0 · 9.9 | 151.3 · 10.6 |
| Logs / bzip2 (Rust) (serial) | 15.7 · 3.0 | 72.3 · 2.4 |
| Logs / bzip2 (C) (serial) | 14.7 · 2.9 | 65.7 · 2.5 |
| MKV / bzip2-turbo | 24.4 · 6.8 | 84.7 · 15.8 |
| MKV / 7zz | 22.2 · 10.1 | 35.0 · 10.5 |
| MKV / bzip2 (Rust) (serial) | 11.0 · 3.0 | 19.6 · 2.4 |
| MKV / bzip2 (C) (serial) | 10.3 · 3.1 | 22.2 · 2.4 |
| ELF / bzip2-turbo | 29.6 · 6.7 | 93.4 · 14.9 |
| ELF / 7zz | 22.9 · 10.1 | 51.8 · 10.6 |
| ELF / bzip2 (Rust) (serial) | 13.8 · 3.0 | 26.6 · 2.4 |
| ELF / bzip2 (C) (serial) | 13.0 · 3.0 | 26.4 · 2.4 |

### Level 1, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 91.4 · 7.8 | 275.4 · 27.4 |
| JSON / 7zz | 49.1 · 13.9 | 115.3 · 10.6 |
| JSON / bzip2 (Rust) (serial) | 16.5 · 3.0 | 55.5 · 2.4 |
| JSON / bzip2 (C) (serial) | 15.6 · 2.9 | 51.5 · 2.4 |
| Repeated text / bzip2-turbo | 170.7 · 7.2 | 871.6 · 26.9 |
| Repeated text / 7zz | 12.9 · 13.9 | 232.7 · 10.5 |
| Repeated text / bzip2 (Rust) (serial) | 7.8 · 3.0 | 181.4 · 2.4 |
| Repeated text / bzip2 (C) (serial) | 6.1 · 3.0 | 164.4 · 2.4 |
| Logs / bzip2-turbo | 113.8 · 7.7 | 336.3 · 27.1 |
| Logs / 7zz | 52.6 · 14.0 | 151.9 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 15.7 · 3.0 | 72.0 · 2.4 |
| Logs / bzip2 (C) (serial) | 14.7 · 2.9 | 65.5 · 2.4 |
| MKV / bzip2-turbo | 48.1 · 11.1 | 160.9 · 28.9 |
| MKV / 7zz | 43.9 · 14.5 | 34.9 · 10.5 |
| MKV / bzip2 (Rust) (serial) | 11.0 · 3.0 | 19.6 · 2.4 |
| MKV / bzip2 (C) (serial) | 10.3 · 3.0 | 22.1 · 2.4 |
| ELF / bzip2-turbo | 56.7 · 10.1 | 155.9 · 25.0 |
| ELF / 7zz | 41.8 · 14.8 | 51.5 · 10.5 |
| ELF / bzip2 (Rust) (serial) | 13.7 · 3.1 | 26.4 · 2.4 |
| ELF / bzip2 (C) (serial) | 13.0 · 3.0 | 26.4 · 2.4 |

### Level 1, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 118.5 · 13.4 | 376.9 · 52.2 |
| JSON / 7zz | 71.3 · 22.4 | 117.5 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 16.5 · 3.1 | 55.5 · 2.4 |
| JSON / bzip2 (C) (serial) | 15.6 · 2.9 | 51.2 · 2.4 |
| Repeated text / bzip2-turbo | 214.5 · 11.7 | 820.7 · 51.1 |
| Repeated text / 7zz | 23.1 · 22.4 | 233.2 · 10.5 |
| Repeated text / bzip2 (Rust) (serial) | 7.8 · 3.0 | 180.6 · 2.4 |
| Repeated text / bzip2 (C) (serial) | 6.1 · 3.0 | 164.2 · 2.4 |
| Logs / bzip2-turbo | 146.2 · 12.6 | 446.0 · 52.4 |
| Logs / 7zz | 77.1 · 22.3 | 151.3 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 15.7 · 3.0 | 72.1 · 2.4 |
| Logs / bzip2 (C) (serial) | 14.7 · 2.9 | 65.6 · 2.4 |
| MKV / bzip2-turbo | 59.7 · 19.2 | 205.9 · 54.6 |
| MKV / 7zz | 52.7 · 23.5 | 35.0 · 10.5 |
| MKV / bzip2 (Rust) (serial) | 11.0 · 3.0 | 19.6 · 2.4 |
| MKV / bzip2 (C) (serial) | 10.3 · 3.0 | 22.1 · 2.4 |
| ELF / bzip2-turbo | 70.4 · 15.9 | 179.0 · 45.8 |
| ELF / 7zz | 53.2 · 23.8 | 50.2 · 10.6 |
| ELF / bzip2 (Rust) (serial) | 13.8 · 3.0 | 26.5 · 2.4 |
| ELF / bzip2 (C) (serial) | 13.0 · 3.0 | 26.3 · 2.4 |

### Level 5, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 25.1 · 7.2 | 79.4 · 7.6 |
| JSON / 7zz | 11.1 · 12.1 | 81.8 · 10.4 |
| JSON / bzip2 (Rust) (serial) | 15.2 · 5.7 | 47.1 · 3.9 |
| JSON / bzip2 (C) (serial) | 14.7 · 5.7 | 44.4 · 3.9 |
| Repeated text / bzip2-turbo | 48.9 · 7.0 | 363.5 · 7.6 |
| Repeated text / 7zz | 5.5 · 11.8 | 256.6 · 10.4 |
| Repeated text / bzip2 (Rust) (serial) | 6.3 · 6.2 | 169.8 · 3.9 |
| Repeated text / bzip2 (C) (serial) | 5.3 · 6.1 | 158.6 · 3.8 |
| Logs / bzip2-turbo | 32.0 · 7.0 | 109.9 · 7.6 |
| Logs / 7zz | 11.8 · 12.1 | 110.7 · 10.3 |
| Logs / bzip2 (Rust) (serial) | 13.9 · 5.7 | 67.3 · 3.9 |
| Logs / bzip2 (C) (serial) | 12.7 · 5.7 | 62.2 · 3.9 |
| MKV / bzip2-turbo | 12.3 · 8.0 | 41.4 · 7.5 |
| MKV / 7zz | 11.0 · 12.8 | 31.7 · 10.3 |
| MKV / bzip2 (Rust) (serial) | 11.4 · 5.6 | 18.1 · 3.9 |
| MKV / bzip2 (C) (serial) | 10.9 · 5.7 | 19.8 · 3.8 |
| ELF / bzip2-turbo | 15.8 · 7.9 | 53.0 · 7.6 |
| ELF / 7zz | 11.4 · 12.9 | 44.6 · 10.4 |
| ELF / bzip2 (Rust) (serial) | 14.8 · 5.6 | 25.8 · 3.9 |
| ELF / bzip2 (C) (serial) | 14.2 · 5.7 | 26.1 · 3.9 |

### Level 5, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 47.3 · 15.7 | 141.2 · 15.1 |
| JSON / 7zz | 20.9 · 19.1 | 114.6 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 14.7 · 5.7 | 48.3 · 4.0 |
| JSON / bzip2 (C) (serial) | 14.2 · 5.6 | 45.1 · 3.9 |
| Repeated text / bzip2-turbo | 91.2 · 14.4 | 579.3 · 14.8 |
| Repeated text / 7zz | 8.8 · 18.4 | 255.5 · 10.4 |
| Repeated text / bzip2 (Rust) (serial) | 6.1 · 6.2 | 169.6 · 3.9 |
| Repeated text / bzip2 (C) (serial) | 5.2 · 6.1 | 156.9 · 3.9 |
| Logs / bzip2-turbo | 59.9 · 15.1 | 186.3 · 14.3 |
| Logs / 7zz | 22.1 · 18.8 | 137.0 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 13.4 · 5.7 | 64.4 · 3.9 |
| Logs / bzip2 (C) (serial) | 12.3 · 5.7 | 60.2 · 3.9 |
| MKV / bzip2-turbo | 23.4 · 20.2 | 75.8 · 16.8 |
| MKV / 7zz | 21.1 · 20.3 | 39.2 · 10.6 |
| MKV / bzip2 (Rust) (serial) | 11.5 · 5.7 | 18.2 · 3.9 |
| MKV / bzip2 (C) (serial) | 10.9 · 5.7 | 20.2 · 3.9 |
| ELF / bzip2-turbo | 29.3 · 18.3 | 88.7 · 14.9 |
| ELF / 7zz | 20.4 · 20.4 | 56.2 · 10.6 |
| ELF / bzip2 (Rust) (serial) | 14.8 · 5.6 | 25.6 · 3.9 |
| ELF / bzip2 (C) (serial) | 14.2 · 5.6 | 26.1 · 3.9 |

### Level 5, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 87.3 · 28.6 | 246.6 · 25.2 |
| JSON / 7zz | 39.7 · 32.4 | 114.6 · 10.5 |
| JSON / bzip2 (Rust) (serial) | 15.2 · 5.6 | 46.2 · 3.9 |
| JSON / bzip2 (C) (serial) | 14.7 · 5.6 | 45.7 · 3.9 |
| Repeated text / bzip2-turbo | 164.6 · 26.2 | 884.8 · 26.8 |
| Repeated text / 7zz | 15.0 · 31.5 | 255.5 · 10.4 |
| Repeated text / bzip2 (Rust) (serial) | 6.2 · 6.2 | 168.0 · 3.9 |
| Repeated text / bzip2 (C) (serial) | 5.2 · 6.0 | 156.5 · 4.0 |
| Logs / bzip2-turbo | 112.3 · 27.5 | 325.9 · 24.4 |
| Logs / 7zz | 42.6 · 32.0 | 140.3 · 10.5 |
| Logs / bzip2 (Rust) (serial) | 13.9 · 5.7 | 67.1 · 3.9 |
| Logs / bzip2 (C) (serial) | 12.7 · 5.7 | 62.2 · 3.9 |
| MKV / bzip2-turbo | 43.7 · 37.7 | 139.2 · 29.0 |
| MKV / 7zz | 39.3 · 35.2 | 39.2 · 10.6 |
| MKV / bzip2 (Rust) (serial) | 11.5 · 5.7 | 18.4 · 3.9 |
| MKV / bzip2 (C) (serial) | 10.9 · 5.6 | 20.0 · 3.9 |
| ELF / bzip2-turbo | 48.5 · 27.5 | 124.9 · 25.2 |
| ELF / 7zz | 37.4 · 34.5 | 55.3 · 10.5 |
| ELF / bzip2 (Rust) (serial) | 14.8 · 5.6 | 25.6 · 3.9 |
| ELF / bzip2 (C) (serial) | 14.1 · 5.7 | 25.7 · 3.9 |

### Level 5, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 105.1 · 53.9 | 225.5 · 46.0 |
| JSON / 7zz | 56.6 · 59.4 | 113.5 · 10.6 |
| JSON / bzip2 (Rust) (serial) | 15.2 · 5.7 | 48.1 · 3.9 |
| JSON / bzip2 (C) (serial) | 14.8 · 5.7 | 44.6 · 3.9 |
| Repeated text / bzip2-turbo | 189.7 · 47.3 | 839.3 · 48.0 |
| Repeated text / 7zz | 16.7 · 57.4 | 257.4 · 10.5 |
| Repeated text / bzip2 (Rust) (serial) | 6.1 · 6.2 | 166.8 · 3.9 |
| Repeated text / bzip2 (C) (serial) | 5.1 · 6.1 | 155.2 · 3.9 |
| Logs / bzip2-turbo | 133.6 · 50.4 | 353.9 · 44.7 |
| Logs / 7zz | 62.5 · 58.3 | 142.1 · 10.6 |
| Logs / bzip2 (Rust) (serial) | 13.9 · 5.6 | 67.0 · 3.9 |
| Logs / bzip2 (C) (serial) | 12.7 · 5.7 | 62.2 · 3.9 |
| MKV / bzip2-turbo | 52.1 · 73.1 | 146.8 · 54.2 |
| MKV / 7zz | 46.1 · 64.6 | 39.3 · 10.6 |
| MKV / bzip2 (Rust) (serial) | 11.5 · 5.7 | 18.5 · 4.0 |
| MKV / bzip2 (C) (serial) | 10.9 · 5.7 | 20.5 · 3.9 |
| ELF / bzip2-turbo | 55.3 · 43.6 | 141.8 · 45.5 |
| ELF / 7zz | 44.9 · 60.1 | 55.3 · 10.5 |
| ELF / bzip2 (Rust) (serial) | 14.7 · 5.7 | 25.7 · 3.9 |
| ELF / bzip2 (C) (serial) | 14.1 · 5.7 | 26.0 · 3.9 |

### Level 9, requested budget 1T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 23.5 · 11.0 | 40.8 · 9.6 |
| JSON / 7zz | 9.8 · 14.7 | 41.2 · 10.6 |
| JSON / bzip2 (Rust) (serial) | 13.6 · 8.3 | 31.3 · 5.5 |
| JSON / bzip2 (C) (serial) | 13.2 · 8.2 | 30.1 · 5.6 |
| Repeated text / bzip2-turbo | 44.8 · 10.4 | 299.2 · 9.6 |
| Repeated text / 7zz | 3.9 · 14.3 | 226.0 · 10.6 |
| Repeated text / bzip2 (Rust) (serial) | 4.6 · 9.1 | 172.6 · 5.5 |
| Repeated text / bzip2 (C) (serial) | 4.0 · 9.2 | 160.3 · 5.4 |
| Logs / bzip2-turbo | 29.9 · 10.7 | 74.2 · 9.6 |
| Logs / 7zz | 10.4 · 14.6 | 73.9 · 10.6 |
| Logs / bzip2 (Rust) (serial) | 12.0 · 8.3 | 50.5 · 5.5 |
| Logs / bzip2 (C) (serial) | 10.8 · 8.3 | 47.6 · 5.5 |
| MKV / bzip2-turbo | 11.7 · 12.3 | 28.0 · 9.6 |
| MKV / 7zz | 9.8 · 16.1 | 22.9 · 10.7 |
| MKV / bzip2 (Rust) (serial) | 10.4 · 8.3 | 15.0 · 5.5 |
| MKV / bzip2 (C) (serial) | 9.8 · 8.3 | 16.3 · 5.5 |
| ELF / bzip2-turbo | 14.5 · 12.2 | 40.4 · 9.5 |
| ELF / 7zz | 10.3 · 15.9 | 35.2 · 10.7 |
| ELF / bzip2 (Rust) (serial) | 13.5 · 8.4 | 22.0 · 5.6 |
| ELF / bzip2 (C) (serial) | 13.0 · 8.3 | 21.8 · 5.6 |

### Level 9, requested budget 2T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 44.7 · 25.6 | 76.5 · 19.0 |
| JSON / 7zz | 18.7 · 24.2 | 48.0 · 10.8 |
| JSON / bzip2 (Rust) (serial) | 13.5 · 8.3 | 31.2 · 5.5 |
| JSON / bzip2 (C) (serial) | 13.2 · 8.3 | 29.7 · 5.4 |
| Repeated text / bzip2-turbo | 84.8 · 23.6 | 470.2 · 18.8 |
| Repeated text / 7zz | 7.1 · 23.2 | 237.4 · 10.6 |
| Repeated text / bzip2 (Rust) (serial) | 4.6 · 9.1 | 171.4 · 5.5 |
| Repeated text / bzip2 (C) (serial) | 4.0 · 9.2 | 159.7 · 5.4 |
| Logs / bzip2-turbo | 57.1 · 25.1 | 133.5 · 18.2 |
| Logs / 7zz | 20.4 · 24.0 | 84.3 · 10.7 |
| Logs / bzip2 (Rust) (serial) | 12.0 · 8.3 | 49.9 · 5.5 |
| Logs / bzip2 (C) (serial) | 10.8 · 8.3 | 46.8 · 5.4 |
| MKV / bzip2-turbo | 22.2 · 34.1 | 50.3 · 21.8 |
| MKV / 7zz | 19.1 · 26.9 | 37.4 · 10.9 |
| MKV / bzip2 (Rust) (serial) | 10.1 · 8.3 | 14.6 · 5.5 |
| MKV / bzip2 (C) (serial) | 9.8 · 8.3 | 15.6 · 5.6 |
| ELF / bzip2-turbo | 25.7 · 25.4 | 62.4 · 19.0 |
| ELF / 7zz | 18.9 · 26.1 | 44.7 · 10.8 |
| ELF / bzip2 (Rust) (serial) | 13.5 · 8.3 | 21.8 · 5.5 |
| ELF / bzip2 (C) (serial) | 12.9 · 8.2 | 21.7 · 5.5 |

### Level 9, requested budget 4T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 82.1 · 48.4 | 135.4 · 33.1 |
| JSON / 7zz | 35.0 · 42.6 | 45.4 · 10.8 |
| JSON / bzip2 (Rust) (serial) | 13.5 · 8.3 | 31.2 · 5.5 |
| JSON / bzip2 (C) (serial) | 13.1 · 8.3 | 30.0 · 5.6 |
| Repeated text / bzip2-turbo | 152.6 · 44.7 | 715.5 · 33.9 |
| Repeated text / 7zz | 12.0 · 41.1 | 227.2 · 10.6 |
| Repeated text / bzip2 (Rust) (serial) | 4.5 · 9.2 | 171.3 · 5.6 |
| Repeated text / bzip2 (C) (serial) | 4.0 · 9.2 | 158.4 · 5.4 |
| Logs / bzip2-turbo | 102.2 · 46.8 | 237.2 · 32.3 |
| Logs / 7zz | 36.7 · 42.1 | 83.5 · 10.7 |
| Logs / bzip2 (Rust) (serial) | 11.9 · 8.3 | 50.8 · 5.5 |
| Logs / bzip2 (C) (serial) | 10.8 · 8.3 | 47.3 · 5.4 |
| MKV / bzip2-turbo | 39.6 · 65.5 | 92.4 · 40.0 |
| MKV / 7zz | 34.3 · 48.1 | 37.5 · 10.8 |
| MKV / bzip2 (Rust) (serial) | 10.1 · 8.3 | 14.6 · 5.5 |
| MKV / bzip2 (C) (serial) | 9.8 · 8.2 | 15.5 · 5.5 |
| ELF / bzip2-turbo | 41.4 · 41.7 | 94.2 · 34.1 |
| ELF / 7zz | 34.4 · 45.7 | 44.3 · 10.8 |
| ELF / bzip2 (Rust) (serial) | 13.5 · 8.3 | 21.8 · 5.5 |
| ELF / bzip2 (C) (serial) | 12.9 · 8.3 | 21.5 · 5.5 |

### Level 9, requested budget 8T

Cells show **MiB/s · peak RSS MiB**, using separate encode/decode medians. Pure Rust and C rows are serial measurements within this requested-budget group.

| Fixture / engine | Encode | Decode |
|---|---:|---:|
| JSON / bzip2-turbo | 92.5 · 80.9 | 162.2 · 61.4 |
| JSON / 7zz | 47.1 · 79.6 | 46.9 · 10.8 |
| JSON / bzip2 (Rust) (serial) | 13.6 · 8.3 | 31.3 · 5.6 |
| JSON / bzip2 (C) (serial) | 13.2 · 8.3 | 29.3 · 5.5 |
| Repeated text / bzip2-turbo | 163.3 · 75.4 | 694.5 · 63.0 |
| Repeated text / 7zz | 11.3 · 76.5 | 228.2 · 10.7 |
| Repeated text / bzip2 (Rust) (serial) | 4.4 · 9.1 | 170.9 · 5.5 |
| Repeated text / bzip2 (C) (serial) | 4.0 · 9.2 | 158.1 · 5.4 |
| Logs / bzip2-turbo | 107.8 · 78.5 | 255.9 · 60.7 |
| Logs / 7zz | 50.1 · 78.8 | 86.2 · 10.8 |
| Logs / bzip2 (Rust) (serial) | 11.9 · 8.3 | 50.9 · 5.5 |
| Logs / bzip2 (C) (serial) | 10.8 · 8.3 | 47.3 · 5.4 |
| MKV / bzip2-turbo | 45.1 · 105.1 | 119.3 · 74.4 |
| MKV / 7zz | 39.8 · 90.5 | 37.3 · 10.8 |
| MKV / bzip2 (Rust) (serial) | 10.1 · 8.3 | 14.5 · 5.5 |
| MKV / bzip2 (C) (serial) | 9.7 · 8.3 | 15.4 · 5.5 |
| ELF / bzip2-turbo | 41.3 · 43.4 | 83.5 · 38.1 |
| ELF / 7zz | 35.0 · 48.0 | 44.3 · 10.8 |
| ELF / bzip2 (Rust) (serial) | 13.4 · 8.4 | 21.6 · 5.5 |
| ELF / bzip2 (C) (serial) | 12.9 · 8.3 | 21.6 · 5.5 |

Raw per-round reports, canonical fixture receipts and build proofs remain outside Git. The build proofs anchor source, isolated backend graphs and executable identities; report self-consistency is not a substitute for those proofs. See [benchmark reproduction](../bench/bzip2-turbo-bench/README.md).

