# Benchmarks

Encode and decode speed for bzip2-turbo 0.2.0 against 7-Zip's `7zz` and the
Rust `bzip2` crate, on one Intel and one ARM machine on AWS, measured on
2026-11-08.

All speeds are in MB/s of uncompressed data (1 MB = 1,000,000 bytes). Higher
is better.

## What was compared

| Engine | What it is |
|---|---|
| turbo | bzip2-turbo 0.2.0, native `Encoder` and `Decoder` |
| 7zz | 7-Zip 26.04, built from source with GCC 13.3 |
| bzip2 (C) | `bzip2` crate 0.6.1 on its C backend (libbz2 1.0.8) |
| bzip2 (Rust) | `bzip2` crate 0.6.1 on its pure-Rust backend (libbz2-rs-sys 0.2.6) |

- **Level** is the block size: level 1, 5 and 9 mean 100 kB, 500 kB and
  900 kB blocks for every engine. turbo and 7zz run one Huffman optimization
  pass, which is turbo's default. At these settings turbo and 7zz produce
  identical compressed bytes.
- **Threads.** turbo and 7zz were run at 1, 2, 4 and 8 threads, each pinned to
  that many CPUs. The `bzip2` crate is single-threaded, so it has one column.
- **Decoding.** Every engine decodes the same stream, the one 7zz wrote.
  Before timing, each engine's output was decoded by every engine and checked
  against the input.
- **Timing.** One warmup, then four rounds with the engines in rotating order.
  Each figure is the median wall time of a whole process, including startup
  and writing the output to `/dev/null`.

## Machines

| | Intel | ARM |
|---|---|---|
| Instance | AWS c7i.2xlarge | AWS c7g.2xlarge |
| CPU | Xeon Platinum 8488C | Graviton3 |
| Cores | 4 cores, 8 threads | 8 cores |
| Memory | 16 GiB | 16 GiB |
| OS | Ubuntu 24.04 | Ubuntu 24.04 |

The Intel machine has only four physical cores, so its 8-thread column runs
two threads per core.

## Inputs

| Input | Size | Content |
|---|---:|---|
| JSON | 16.8 MB | Generated JSON records with varied values |
| Logs | 16.8 MB | Generated structured log lines |
| Linux binary | 3.9 MB | A Linux executable built with Go 1.27 |
| Video (MKV) | 16.4 MB | 150 seconds of H.264/AAC [Big Buck Bunny](https://peach.blender.org/about/) (© 2008 Blender Foundation, [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/)), remuxed to Matroska |
| Repeated text | 16.8 MB | One 71-byte sentence repeated; a worst case for block sorting |

## Intel (c7i.2xlarge)

### Encode

| MB/s | Level | bzip2 (C) | bzip2 (Rust) | 7zz 1T | 7zz 2T | 7zz 4T | 7zz 8T | turbo 1T | turbo 2T | turbo 4T | turbo 8T |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| JSON | 1 | 17 | 18 | 14 | 26 | 51 | 75 | 26 | 49 | 96 | 124 |
| JSON | 5 | 15 | 16 | 12 | 22 | 42 | 59 | 26 | 50 | 92 | 110 |
| JSON | 9 | 14 | 14 | 10 | 20 | 37 | 49 | 25 | 47 | 86 | 97 |
| Logs | 1 | 16 | 17 | 15 | 28 | 55 | 81 | 32 | 61 | 119 | 153 |
| Logs | 5 | 13 | 15 | 12 | 23 | 45 | 66 | 34 | 63 | 118 | 140 |
| Logs | 9 | 11 | 13 | 11 | 21 | 38 | 53 | 31 | 60 | 107 | 113 |
| Linux binary | 1 | 14 | 14 | 12 | 24 | 44 | 56 | 16 | 31 | 59 | 74 |
| Linux binary | 5 | 15 | 16 | 12 | 21 | 39 | 47 | 17 | 31 | 51 | 58 |
| Linux binary | 9 | 14 | 14 | 11 | 20 | 36 | 37 | 15 | 27 | 43 | 43 |
| Video (MKV) | 1 | 11 | 12 | 12 | 23 | 46 | 55 | 13 | 26 | 50 | 63 |
| Video (MKV) | 5 | 11 | 12 | 12 | 22 | 41 | 48 | 13 | 25 | 46 | 55 |
| Video (MKV) | 9 | 10 | 11 | 10 | 20 | 36 | 42 | 12 | 23 | 42 | 47 |
| Repeated text | 1 | 6.4 | 8.2 | 3.5 | 6.9 | 14 | 24 | 49 | 94 | 179 | 225 |
| Repeated text | 5 | 5.6 | 6.6 | 5.8 | 9.2 | 16 | 18 | 51 | 96 | 173 | 199 |
| Repeated text | 9 | 4.2 | 4.8 | 4.1 | 7.4 | 13 | 12 | 47 | 89 | 160 | 171 |

### Decode

| MB/s | Level | bzip2 (C) | bzip2 (Rust) | 7zz 1T | 7zz 2T | 7zz 4T | 7zz 8T | turbo 1T | turbo 2T | turbo 4T | turbo 8T |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| JSON | 1 | 54 | 58 | 92 | 122 | 121 | 123 | 90 | 161 | 289 | 395 |
| JSON | 5 | 47 | 49 | 86 | 120 | 120 | 119 | 83 | 148 | 259 | 236 |
| JSON | 9 | 32 | 33 | 43 | 50 | 48 | 49 | 43 | 80 | 142 | 170 |
| Logs | 1 | 69 | 76 | 115 | 159 | 159 | 159 | 113 | 198 | 353 | 468 |
| Logs | 5 | 65 | 71 | 116 | 144 | 147 | 149 | 115 | 195 | 342 | 371 |
| Logs | 9 | 50 | 53 | 77 | 88 | 88 | 90 | 78 | 140 | 249 | 268 |
| Linux binary | 1 | 28 | 28 | 49 | 54 | 54 | 53 | 57 | 98 | 163 | 188 |
| Linux binary | 5 | 27 | 27 | 47 | 59 | 58 | 58 | 56 | 93 | 131 | 149 |
| Linux binary | 9 | 23 | 23 | 37 | 47 | 46 | 46 | 42 | 65 | 99 | 88 |
| Video (MKV) | 1 | 23 | 21 | 35 | 37 | 37 | 37 | 49 | 89 | 169 | 216 |
| Video (MKV) | 5 | 21 | 19 | 33 | 41 | 41 | 41 | 43 | 79 | 146 | 154 |
| Video (MKV) | 9 | 17 | 16 | 24 | 39 | 39 | 39 | 29 | 53 | 97 | 125 |
| Repeated text | 1 | 174 | 191 | 245 | 250 | 244 | 245 | 380 | 587 | 914 | 861 |
| Repeated text | 5 | 166 | 178 | 269 | 268 | 268 | 270 | 381 | 607 | 928 | 880 |
| Repeated text | 9 | 168 | 181 | 237 | 249 | 238 | 239 | 314 | 493 | 750 | 728 |

### Peak memory, JSON, level 9

| MB | Encode 1T | Encode 8T | Decode 1T | Decode 8T |
|---|---:|---:|---:|---:|
| bzip2 (C) | 8.6 | 8.7 | 5.9 | 5.8 |
| bzip2 (Rust) | 8.7 | 8.7 | 5.8 | 5.9 |
| 7zz | 15 | 83 | 11 | 11 |
| turbo | 12 | 85 | 10 | 64 |

## ARM (c7g.2xlarge)

### Encode

| MB/s | Level | bzip2 (C) | bzip2 (Rust) | 7zz 1T | 7zz 2T | 7zz 4T | 7zz 8T | turbo 1T | turbo 2T | turbo 4T | turbo 8T |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| JSON | 1 | 16 | 17 | 14 | 27 | 53 | 104 | 24 | 48 | 94 | 179 |
| JSON | 5 | 15 | 16 | 11 | 22 | 42 | 74 | 23 | 45 | 84 | 143 |
| JSON | 9 | 14 | 14 | 10 | 20 | 38 | 60 | 21 | 40 | 74 | 116 |
| Logs | 1 | 15 | 16 | 15 | 29 | 57 | 109 | 29 | 58 | 113 | 216 |
| Logs | 5 | 12 | 14 | 12 | 24 | 47 | 85 | 28 | 55 | 103 | 177 |
| Logs | 9 | 11 | 12 | 11 | 22 | 40 | 63 | 25 | 48 | 87 | 135 |
| Linux binary | 1 | 13 | 12 | 12 | 24 | 44 | 80 | 16 | 31 | 59 | 101 |
| Linux binary | 5 | 13 | 13 | 11 | 20 | 37 | 61 | 15 | 28 | 47 | 67 |
| Linux binary | 9 | 13 | 12 | 10 | 20 | 36 | 36 | 14 | 25 | 39 | 39 |
| Video (MKV) | 1 | 10.0 | 8.2 | 11 | 21 | 42 | 81 | 12 | 24 | 48 | 93 |
| Video (MKV) | 5 | 10 | 8.4 | 9.9 | 19 | 36 | 67 | 11 | 22 | 42 | 75 |
| Video (MKV) | 9 | 9.5 | 8.0 | 9.2 | 18 | 33 | 53 | 10 | 20 | 37 | 58 |
| Repeated text | 1 | 5.9 | 7.2 | 4.3 | 8.5 | 17 | 33 | 44 | 86 | 169 | 314 |
| Repeated text | 5 | 4.8 | 5.6 | 3.3 | 5.3 | 9.5 | 17 | 43 | 85 | 156 | 256 |
| Repeated text | 9 | 4.4 | 5.2 | 2.4 | 3.7 | 6.1 | 10 | 43 | 83 | 148 | 222 |

### Decode

| MB/s | Level | bzip2 (C) | bzip2 (Rust) | 7zz 1T | 7zz 2T | 7zz 4T | 7zz 8T | turbo 1T | turbo 2T | turbo 4T | turbo 8T |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| JSON | 1 | 58 | 61 | 99 | 163 | 163 | 163 | 101 | 178 | 323 | 517 |
| JSON | 5 | 34 | 34 | 44 | 53 | 54 | 54 | 45 | 84 | 157 | 252 |
| JSON | 9 | 28 | 28 | 35 | 40 | 40 | 41 | 35 | 64 | 107 | 135 |
| Logs | 1 | 73 | 81 | 122 | 181 | 182 | 182 | 124 | 218 | 388 | 607 |
| Logs | 5 | 50 | 53 | 69 | 82 | 82 | 83 | 71 | 130 | 239 | 396 |
| Logs | 9 | 39 | 41 | 48 | 60 | 55 | 54 | 47 | 99 | 153 | 211 |
| Linux binary | 1 | 34 | 32 | 53 | 65 | 65 | 65 | 53 | 87 | 145 | 197 |
| Linux binary | 5 | 27 | 26 | 38 | 55 | 55 | 57 | 39 | 60 | 92 | 126 |
| Linux binary | 9 | 23 | 21 | 28 | 38 | 39 | 46 | 28 | 42 | 62 | 77 |
| Video (MKV) | 1 | 29 | 24 | 38 | 45 | 45 | 45 | 37 | 65 | 126 | 216 |
| Video (MKV) | 5 | 21 | 18 | 25 | 44 | 45 | 45 | 25 | 44 | 84 | 140 |
| Video (MKV) | 9 | 16 | 15 | 18 | 36 | 34 | 37 | 17 | 29 | 51 | 78 |
| Repeated text | 1 | 141 | 157 | 208 | 219 | 220 | 219 | 364 | 580 | 947 | 1225 |
| Repeated text | 5 | 140 | 155 | 208 | 217 | 218 | 218 | 365 | 636 | 1022 | 1325 |
| Repeated text | 9 | 139 | 155 | 208 | 216 | 216 | 217 | 367 | 649 | 1013 | 1270 |

### Peak memory, JSON, level 9

| MB | Encode 1T | Encode 8T | Decode 1T | Decode 8T |
|---|---:|---:|---:|---:|
| bzip2 (C) | 8.3 | 8.3 | 5.2 | 5.2 |
| bzip2 (Rust) | 8.3 | 8.3 | 5.3 | 5.3 |
| 7zz | 15 | 83 | 10 | 11 |
| turbo | 11 | 84 | 9.4 | 65 |

## Reading the results

- **Single-threaded encoding.** turbo is the fastest encoder at one thread on
  every input, level and machine. On JSON at level 9 it encodes at 25 MB/s on
  Intel and 21 MB/s on ARM, where libbz2 manages 14.
- **Threaded encoding.** turbo and 7zz both scale with threads. turbo stays
  ahead at every thread count.
- **Repeated text.** This input defeats the block sorters in 7zz and libbz2,
  which drop to 2 to 8 MB/s. turbo's sorter keeps it at 43 to 51 MB/s on one
  thread.
- **Single-threaded decoding.** turbo and 7zz are level, except that turbo
  is faster on repeated text on both machines and on the binary and video on
  Intel.
- **Threaded decoding.** 7zz gains little past two threads. turbo keeps
  scaling: on ARM it decodes JSON at level 1 at 517 MB/s on 8 threads, against
  163 MB/s for 7zz.
- **Small inputs.** The Linux binary is 3.9 MB, only five blocks at level 9,
  so there is little to split up: 4 and 8 threads encode it at the same
  speed.
- **Intel at 8 threads.** With two threads per core, some 8-thread figures
  are slower than the 4-thread ones.
- **Memory.** Threaded decoding uses about 8 MB per worker: turbo peaks at
  about 65 MB on 8 threads, while 7zz stays near 11 MB. Encoding memory is
  about the same for turbo and 7zz.

## Running the benchmarks

The harness is a Go program in
[`bench/bzip2-turbo-bench`](../bench/bzip2-turbo-bench/README.md); its README
has the full steps. In outline:

1. Build turbo's `codec-bench` example in release mode. Build the two `bzip2`
   crate drivers separately, never with both features in one build:

   ```sh
   cargo build --locked --release --example codec-bench
   cargo build --locked --release --manifest-path tools/bzip2-reference/Cargo.toml --features pure --target-dir target/reference-pure
   cargo build --locked --release --manifest-path tools/bzip2-reference/Cargo.toml --features c --target-dir target/reference-c
   ```

2. Build the pinned 7-Zip with `go run .github/scripts/tool.go oracle`.
3. Generate the five inputs once with `go run . fixture`, and copy them with
   their receipts to every machine you test, so each one compresses the same
   bytes.
4. Run `go run . suite` on an otherwise idle machine. Keep inputs, reports
   and profiles outside the checkout.

All three Rust programs use the same profile: optimization level 3, fat LTO,
one codegen unit, stripped, and no `target-cpu` override.

The harness defaults to the block-size settings used here. Its
`--settings sevenzip-level` option reproduces older runs that used 7-Zip's
own level defaults, where the same level number does not mean the same
settings in every engine. Don't compare those runs with these.

## Build and input identities

Source: bzip2-turbo `3225e57cadfc266765e8f46d851373091c87de53`, Rust 1.97.1,
Go 1.23.12 for the harness, 7-Zip source
[`9128b80`](https://github.com/ip7z/7zip/tree/9128b80e3a471108678e2b3ec3c985861c8d7b0f)
built with its stock `makefile.gcc`.

| Input | Bytes | SHA-256 |
|---|---:|---|
| JSON | 16,777,216 | `f7858958c4a1f4a42ba26006a058fa41c657fcd7f2e2d321300d517398e9530e` |
| Logs | 16,777,222 | `170a8ba5218e7c1f67a3ba9031daa904540196a8aa972425b19c03ae5931ecd1` |
| Linux binary | 3,903,413 | `5d993bf86e336c2c54ee67c0c6be15fc1418e4e16d581a87d256823f467bfa97` |
| Video (MKV) | 16,360,253 | `a88533696bc7ff1bb9847af06ccf37aafa7c781ea7d94eefbd7c8ff10d34525a` |
| Repeated text | 16,777,216 | `8befb6b762230bc79630499c8086a8137f0a0078c688b51348115699180fbece` |

| Executable | Intel SHA-256 | ARM SHA-256 |
|---|---|---|
| turbo | `ec8fa9c00151890b04c386451119bc76629d8a8dd67765a2da406b8c8849e1ab` | `ec91243ccd17bb1d514d88b6b8554cb08286c8742e7495e630c2939e2d668e1c` |
| 7zz | `4d7d10d75af4e060adb57c20e40d24317914321c4d0e916f49897642383b61a2` | `765161e82b3e43a23a4fcd2492723a8154d640aa0b4abf5823787978b3368a1f` |
| bzip2 (Rust) | `d967d4035a69cfbc7c4fdb5ffbe7637b6f6ee8ba979acd1475f5992db69ee753` | `1b827f8cbde07bcb328cca02d49144c02c659272fcd624c2c5dd1ce80dce794d` |
| bzip2 (C) | `27c898b2c03e82face00e352d356e0b5ed5c58c87f7010ac2fe2eeb0f865aced` | `df9ac406e1ff49ddf95900534054b8ca6acbacb3ac1108c8344ffb9a42492ca1` |

Raw per-round reports are kept outside the repository.

## Profiling findings

Notes from profiling turbo against the 7-Zip code it was translated from.
They come from profiles and operation counts, not from the timings above.

- **Huffman table selection.** On Apple silicon, 14% of JSON encoding
  samples went to choosing which of six Huffman tables codes each group of 50
  symbols. turbo now sums all six table costs at once, packed into one 64-bit
  integer. It picks the same tables and writes the same bytes.
- **Block sorting on repetitive input.** On the repeated-sentence input, 82%
  of the sorter's rank reads were spent in partition passes that moved
  nothing. The sorter now finds each group's rank range once and skips those
  passes, with the same sort order.
- **CRC.** turbo's checksums come from `crc-turbo`, whose vectorized CRC is
  around 90 times faster than 7-Zip's table-driven loop on 4 KiB and larger
  buffers. 7-Zip's loop is faster on single-byte calls.
