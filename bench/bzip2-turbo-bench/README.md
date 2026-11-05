# BZip2 process benchmark harness

Standard-library-only Go harness with `toolchain`, `run`, and `suite` commands.
`legacy-corpus DESTINATION --mib 32` and `legacy-compare --codec EXE --oracle
EXE --input FILE --results JSON` preserve the historical two-process tools,
including byte-identical xorshift corpora and the original JSON report keys.
The latter accepts the historical level, block-size, passes, runs, warmups,
hardware, revision, profile, darwin-resources and require-parity options.
Platform identity values use Go OS/architecture names. These legacy commands
keep their candidate/reference ratio direction; four-contender reports use
reference/ours. All raw outputs stay outside the checkout.
Four contenders run in rotating order: 7zz, turbo, Rust bzip2 with its pure Rust
backend, and Rust bzip2 with its C backend. Every encoded output is decoded by
every contender before timing. Native turbo output must match 7zz byte for byte;
other encoders need decoded-byte parity, not compressed-byte identity.

The main `text` corpus is exact-sized valid JSON with recurring attribute
keys and diverse seeded values: variable text, numbers, booleans, arrays and
nested objects. A fixed xorshift64* seed drives values continuously; no global
text block repeats. Generation keeps one record and a 64 KiB buffer in memory.
Final string padding is shorter than the next record. Minimum size is 256 bytes.
To keep an external fixture: `go run . json-corpus --output /tmp/text.json
--bytes 33554432`. Historical `legacy-corpus` text remains repeated-sentence
periodic stress for reproducing old evidence, not the main text workload.

The expanded comparison uses five explicitly selected fixtures:

| Kind | Workload | Extent |
| --- | --- | --- |
| `json` | The varied JSON generator above (`text` remains its suite alias) | Exact requested bytes |
| `repeated-text` | One continuous 71-byte sentence, retained as periodic stress | Exact requested bytes |
| `logs` | Seeded logfmt records with varied UTC times, severity, services, request IDs, paths, status, sizes and messages | Complete records, at least the requested bytes |
| `mkv` | Real Big Buck Bunny H.264/AAC footage, stream-copy remuxed to Matroska | Fixed 150-second excerpt; actual container bytes |
| `elf` | A real Go standard-library JSON event summarizer, cross-compiled for Linux AMD64 v1 | Actual linked ELF bytes |

Generate each once outside the checkout, keeping its JSON receipt:

```sh
mkdir -p /tmp/bzip2-fixtures
go run . fixture --kind json --output /tmp/bzip2-fixtures/json.json --bytes 16777216 > /tmp/bzip2-fixtures/json.receipt.json
go run . fixture --kind repeated-text --output /tmp/bzip2-fixtures/repeated.txt --bytes 16777216 > /tmp/bzip2-fixtures/repeated.receipt.json
go run . fixture --kind logs --output /tmp/bzip2-fixtures/logs.log --bytes 16777216 > /tmp/bzip2-fixtures/logs.receipt.json
go run . fixture --kind mkv --source-file /tmp/bbb-source.zip --output /tmp/bzip2-fixtures/film.mkv > /tmp/bzip2-fixtures/film.receipt.json
go run . fixture --kind elf --output /tmp/bzip2-fixtures/program.elf > /tmp/bzip2-fixtures/program.receipt.json
```

The primary MKV source is the pinned official Blender ZIP or its verified MP4
member. Without `--source-file`, downloading requires explicit `--download-source`;
no generator installs tools or downloads a toolchain. Only the exact ZIP member
is extracted, with bounded sizes and SHA-256 checks for archive, MP4 and canonical
MKV. The excerpt starts at 30 seconds, lasts 150 seconds, and copies video/audio
without re-encoding. It retains attribution in container metadata and the receipt:
Copyright 2008 Blender Foundation / www.bigbuckbunny.org;
[CC BY 3.0](https://creativecommons.org/licenses/by/3.0/).
See the [film project](https://peach.blender.org/about/) for source attribution.
The changed work is a trimmed, remuxed excerpt with replaced metadata.
The canonical MKV SHA-256 is
`a88533696bc7ff1bb9847af06ccf37aafa7c781ea7d94eefbd7c8ff10d34525a`.
The canonical remux was verified with FFmpeg 8.1.1. A different output is
rejected; use canonical bytes or a matching FFmpeg build.

`--kind mkv --source noise` retains an optional synthetic alternative: a fixed
xorshift64* stream becomes 256x256 grayscale frames, FFV1 level 3 with range
coding, then a metadata-free Matroska container. Explicit bitexact flags, one
thread and disabled CPU dispatch avoid incidental run-to-run variation. ffprobe
checks codec, dimensions, frame count and actual byte extent; a full FFmpeg
lossless decode must match the seeded raw SHA-256. Both media modes undergo full
decode validation. Neither container nor ELF is truncated or padded to a target.

ELF generation uses existing Go with `GOTOOLCHAIN=local`, no external modules,
CGO disabled, Linux AMD64 v1, `-trimpath`, `-buildvcs=false` and an empty build ID.
The Go version, fixed build flags and embedded program source hash are recorded;
reproducing an ELF requires that same Go version. Tests build in two independent
temporary directories and require equal hashes, valid ELF text/load segments,
no build ID or private build paths, and material compressibility. The generated
ELF is parsed, never executed by the generator.

Copy the canonical fixture **and its receipt** unchanged to each benchmark host.
Receipts record actual size/hash and applicable seeds, generator versions,
commands, media validation and public-source licensing. Bind them to timed runs:

```sh
go run . run --input /tmp/bzip2-fixtures/film.mkv --fixture-manifest /tmp/bzip2-fixtures/film.receipt.json --level 5 --threads 1 --repeats 4 --out /tmp/film-run.json
```

The receipt SHA-256 and actual bytes must match before measurement. Imported ELF
is parsed and imported MKV needs a bounded Matroska header check; the primary
movie also must match its pinned canonical SHA-256 and source attribution.
Receipts record provenance, not a trusted attestation of arbitrary user files.
No FFmpeg installation is required to benchmark copied canonical inputs. Reports retain
fixture provenance and `reference_encoded_fraction` / `ours_encoded_fraction`
(encoded bytes divided by actual input bytes). MKV rows require a measured 7zz
fraction of at least 0.99; ELF rows require at most 0.90. These are measured
compressibility checks, not assumptions based on file extensions. Compare the
same fixture hashes across architectures rather than regenerating with different
FFmpeg or Go releases. Raw fixture bytes, generated sources and binaries remain
outside Git; only generator source and tests are checked in.

Suite defaults stay `text,random,periodic,runs` and do not require extra tools.
The generic `periodic` corpus is a 256-byte pattern, distinct from the 71-byte
`repeated-text` stress case. To generate the expanded suite explicitly, use
`--corpora json,repeated-text,logs,mkv,elf --mkv-source-file /tmp/bbb-source.zip`;
`--mkv-source noise` explicitly selects synthetic media instead. Canonical
single-generation plus receipt-bound `run` commands is preferred across hosts.

Build the native example and both isolated reference builds as documented in
`../../tools/bzip2-reference/README.md`, then run from this directory:

```sh
cargo build --manifest-path ../../Cargo.toml --release --example codec-bench
go run . toolchain --sevenzip 7zz
go run . suite --sevenzip 7zz --size-kib 16 --levels 1,5,9 --threads 1,2 --api both --repeats 4 --out /tmp/bzip2-smoke.json
# Full matrix, only when explicitly measuring performance on an idle host:
go run . suite --sevenzip 7zz --size-kib 32768 --levels 1,5,9 --threads 1,2,4,8 --api both --repeats 4 --out /tmp/bzip2-matrix.json
go run . run --sevenzip 7zz --input /tmp/corpus.bin --threads 1 --out /tmp/bzip2-run.json
```

The turbo example and both reference builds share release fat LTO, one codegen
unit, optimization level 3 and stripping. Keep the Rust toolchain, profile
overrides and any target-CPU override identical across those three builds; the
documented commands use the default target CPU baseline without
`target-cpu=native`. Record C-backend and 7zz compiler settings separately.
See the reference-build README for explicit equivalent environment overrides.

`--driver`, `--pure`, and `--c` select alternate driver builds; `--repo` selects
the source identity root. No tools are built or downloaded automatically.
`--list` prints the matrix without requiring tools. Corpora are deterministic
text, random, periodic and runs. Compatibility turbo is serial and skipped for
thread counts above one. Pure Rust and C references always run one real thread;
`reference_threads` records this even when native turbo and 7zz use more threads.

Default `--settings block-size` gives the setting index an explicit meaning:
`--levels 1,5,9` selects 100,000, 500,000 and 900,000-byte blocks. Native turbo
and 7zz receive that block size and one optimization pass explicitly; standard
bzip2 uses its corresponding compression setting and single standard pipeline.
Every output header is checked against its declared effective block size.
Matching block/pass counts do not make the encoders' algorithms identical.

Optional `--settings sevenzip-level` retains historical native 7zip defaults.
For example native level 5 uses a 900,000-byte block while standard bzip2 level 5
uses 500,000; native level 9 uses seven recursive optimization passes while the
standard pipeline uses one. These rows explicitly report each contender's
`block_bytes`, `optimization_passes`, policy and whether block/pass counts match.
Compatibility turbo always uses standard block sizing and one pass, and 7zz is
aligned to it even under the historical setting policy. Such historical rows
must not be interpreted as equal-setting performance comparisons.

At least three measured rounds are required; four rounds balance all four
starting positions. Warmups are discarded. Each encode/decode scenario produces
three reference-versus-turbo rows from the same four-contender timing rounds.
Decode timings use the same 7zz stream for all contenders. All timed codecs
write to the operating-system null device, recorded in `OutputSink`; Rust
drivers open that output path instead of using their special `-` in-memory
sink. This retains buffering and output writes for every contender. Earlier
reports without `OutputSink` used the special Rust sink and must remain
separate when comparing complete-process results. Reports contain raw
wall/CPU samples, median wall and peak RSS, reference-over-turbo wall/RSS ratios,
encoded sizes and identities, binary identities, and a digest of codec, driver,
reference manifests/lockfiles and Go harness source. Values above one favor turbo.
The optional turbo compatibility API retains the turbo contender identity.

Linux schema-5 reports require `RSSMethod: gnu-time-child-v1`. Each command is
launched through the installed GNU `/usr/bin/time`; RSS and CPU come from that
reporter's **own child report**, written to a private 0600 file in a 0700 temporary
directory. Linux KiB are converted to bytes. Do not read the Go `ProcessState`
RSS of either codec or reporter: Linux can retain the Go parent's pre-exec
address-space high-water mark across exec, creating an artificial RSS floor.
GNU time's command resource accounting is not simultaneous aggregate tree RSS;
these codec commands do not launch subprocess workloads.

Wall timing remains Go's high-resolution monotonic clock and includes GNU time
startup, codec startup and I/O. GNU time's codec user/system CPU fields have
hundredth-second precision and may be zero for short commands. Codec errors
remain failures. Missing, malformed, zero or overflowing RSS fails closed; no
fallback uses the old direct-rusage measurement. A deadline kills the reporter
and codec's entire task-owned process group. Temporary reports are removed.

Darwin retains native byte-unit child rusage. Unsupported platforms omit RSS.
Schema-4 Linux RSS from direct Go rusage is invalid for codec-memory comparisons;
preserve those raw reports with that label, never subtract a floor or replace
fields in place. Corrected runs require the new method and source provenance.
The Linux regression keeps 64/128 MiB resident in the Go parent, verifies a small
child stays small, and also checks a genuinely large child and timeout teardown:
`go test -v -run 'TestGNUTimeReport|TestLinuxRSS|TestLinuxReporter|TestLinuxProcess' .`.
Hardware counters and host idleness are not assessed.
Tiny inputs only verify the harness and correctness; they are not performance
evidence. Raw reports and corpus files must remain outside the repository,
including through symlink aliases. No optimization is performed by this harness.
