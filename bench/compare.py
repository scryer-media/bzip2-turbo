"""Cross-platform raw-stream parity and process timing; run on an idle host."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import tempfile
import time


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(1024 * 1024):
            value.update(block)
    return value.hexdigest()


DARWIN_COUNTERS = {
    "instructions retired": "instructions_retired",
    "cycles elapsed": "cycles_elapsed",
    "maximum resident set size": "maximum_resident_set_size_bytes",
    "peak memory footprint": "peak_memory_footprint_bytes",
    "voluntary context switches": "voluntary_context_switches",
    "involuntary context switches": "involuntary_context_switches",
    "page reclaims": "page_reclaims",
    "page faults": "page_faults",
}
DARWIN_POSITIVE_COUNTERS = (
    "instructions_retired", "cycles_elapsed",
    "maximum_resident_set_size_bytes", "peak_memory_footprint_bytes",
)


def parse_darwin_resources(raw):
    parsed = {}
    for line in raw.splitlines():
        match = re.fullmatch(r"\s*(\d+)\s+(.+?)\s*", line)
        if match and match[2] in DARWIN_COUNTERS:
            key = DARWIN_COUNTERS[match[2]]
            if key in parsed:
                raise RuntimeError(f"duplicate Darwin resource counter: {key}")
            parsed[key] = int(match[1])
    missing = set(DARWIN_COUNTERS.values()) - parsed.keys()
    if missing:
        raise RuntimeError(f"missing Darwin resource counters: {', '.join(sorted(missing))}")
    unavailable = [key for key in DARWIN_POSITIVE_COUNTERS if parsed[key] <= 0]
    if unavailable:
        raise RuntimeError(f"unavailable Darwin resource counters: {', '.join(unavailable)}")
    return parsed


def measured_parity(measurements):
    pairs = {}
    for item in measurements:
        key = (item["mode"], item["threads"])
        pair = pairs.setdefault(key, {})
        implementation = item["implementation"]
        if implementation in pair:
            raise RuntimeError(f"duplicate parity measurement: {key}, {implementation}")
        pair[implementation] = item["median_seconds"]
    failures = []
    for (mode, threads), pair in pairs.items():
        if set(pair) != {"reference", "candidate"}:
            raise RuntimeError(f"incomplete parity measurements: {mode}, {threads}")
        if pair["candidate"] > pair["reference"]:
            failures.append({"mode": mode, "threads": threads,
                             "reference_median_seconds": pair["reference"],
                             "candidate_median_seconds": pair["candidate"]})
    return {
        "passed": not failures,
        "comparisons": len(pairs),
        "criterion": "candidate median_seconds <= reference median_seconds, using unrounded values and no tolerance",
        "host_quality": "not assessed: the script cannot establish host idleness or measurement stability",
        "failures": failures,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codec", required=True, type=Path)
    parser.add_argument("--oracle", required=True, type=Path)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--results", required=True, type=Path)
    parser.add_argument("--level", type=int, default=5, choices=range(1, 10))
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--block-size", type=int)
    parser.add_argument("--passes", type=int)
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--hardware", default=platform.processor() or platform.machine())
    parser.add_argument("--revision", default="unspecified")
    parser.add_argument("--profile", default="release")
    parser.add_argument("--darwin-resources", action="store_true",
                        help="capture /usr/bin/time -l counters for both competitors; wall time includes the wrapper")
    parser.add_argument("--require-parity", action="store_true",
                        help="exit nonzero after writing all results if any unrounded candidate median exceeds its reference")
    args = parser.parse_args()
    if args.runs < 3:
        parser.error("at least three runs are required")
    if args.warmups < 1:
        parser.error("at least one warmup is required")
    if args.darwin_resources and platform.system() != "Darwin":
        parser.error("--darwin-resources requires Darwin /usr/bin/time -l")
    repository = Path(__file__).resolve().parents[3]
    if args.results.resolve().is_relative_to(repository):
        parser.error("raw results must be outside the repository")
    report = {"platform": platform.platform(), "machine": platform.machine(),
              "hardware": args.hardware, "logical_cpus": os.cpu_count(),
              "revision": args.revision, "profile": args.profile,
              "warmups": args.warmups, "order": "alternating per repetition",
              "codec_sha256": digest(args.codec), "oracle_sha256": digest(args.oracle),
              "input_bytes": args.input.stat().st_size, "input_sha256": digest(args.input),
              "level": args.level, "runs": args.runs, "block_size": args.block_size,
              "passes": args.passes, "measurements": [], "ratios": []}
    if args.darwin_resources:
        report["darwin_resources"] = {
            "collector": "/usr/bin/time -l -o FILE",
            "wall_measurement": "perf_counter around /usr/bin/time and its child; includes wrapper startup and counter reporting",
            "memory_unit": "bytes",
            "counter_scope": "command process as reported by Darwin time; warmups excluded from recorded runs",
        }
    candidate_options = [str(args.block_size) if args.block_size is not None else "-",
                         str(args.passes) if args.passes is not None else "-"]
    # Corpus and temporary compressed bytes remain outside the repository.
    with tempfile.TemporaryDirectory(prefix="bzip2-parity-") as temporary:
        root = Path(temporary)
        packed, candidate, decoded = [root / name for name in ("oracle.bz2", "candidate.bz2", "decoded")]
        for threads in (1, 2, 4, 8):
            oracle_encode = [str(args.oracle), "a", "-tbzip2", f"-mx={args.level}", f"-mmt={threads}", "-si", "-so", "stream.bz2"]
            if args.block_size is not None:
                oracle_encode.append(f"-md={args.block_size}b")
            if args.passes is not None:
                oracle_encode.append(f"-mpass={args.passes}")
            oracle_decode = [str(args.oracle), "x", "-tbzip2", f"-mmt={threads}", "-si", "-so"]
            common = [str(args.codec), "encode", str(args.level), str(threads), str(args.input)]
            with args.input.open("rb") as source, packed.open("wb") as target:
                subprocess.run(oracle_encode, stdin=source, stdout=target, check=True)
            subprocess.run(common + [str(candidate)] + candidate_options, check=True)
            if digest(candidate) != digest(packed):
                raise RuntimeError(f"encoded bytes differ at {threads} threads")
            subprocess.run([str(args.codec), "decode", str(args.level), str(threads), str(packed), str(decoded)], check=True)
            if digest(decoded) != report["input_sha256"]:
                raise RuntimeError(f"decoded bytes differ at {threads} threads")
            for mode in ("encode", "decode"):
                source_path = args.input if mode == "encode" else packed
                commands = {}
                for implementation in ("reference", "candidate"):
                    command = (oracle_encode if mode == "encode" else oracle_decode) if implementation == "reference" else [str(args.codec), mode, str(args.level), str(threads), str(source_path), "-"]
                    if implementation == "candidate":
                        command += candidate_options
                    commands[implementation] = command
                timings = {"reference": [], "candidate": []}
                resources = {"reference": [], "candidate": []}
                for repetition in range(args.warmups + args.runs):
                    order = ("reference", "candidate") if repetition % 2 == 0 else ("candidate", "reference")
                    for implementation in order:
                        command = commands[implementation]
                        if args.darwin_resources:
                            resource_path = root / f"resources-{threads}-{mode}-{implementation}-{repetition}.txt"
                            command = ["/usr/bin/time", "-l", "-o", str(resource_path)] + command
                        with source_path.open("rb") as source, open(os.devnull, "wb") as target:
                            start = time.perf_counter()
                            subprocess.run(command, stdin=source, stdout=target, check=True)
                            elapsed = time.perf_counter() - start
                        if args.darwin_resources:
                            raw = resource_path.read_text()
                            resource = {"raw": raw, "parsed": parse_darwin_resources(raw)}
                        if repetition >= args.warmups:
                            timings[implementation].append(elapsed)
                            if args.darwin_resources:
                                resources[implementation].append(resource)
                for implementation in ("reference", "candidate"):
                    median = statistics.median(timings[implementation])
                    item = {"mode": mode, "implementation": implementation, "threads": threads,
                            "seconds": timings[implementation], "median_seconds": median,
                            "mib_per_second": report["input_bytes"] / median / (1024 * 1024)}
                    if args.darwin_resources:
                        item["darwin_resource_runs"] = resources[implementation]
                        item["darwin_resource_medians"] = {
                            key: statistics.median(run["parsed"][key] for run in resources[implementation])
                            for key in DARWIN_COUNTERS.values()
                        }
                        item["maximum_rss_bytes"] = max(
                            run["parsed"]["maximum_resident_set_size_bytes"] for run in resources[implementation])
                        item["maximum_peak_memory_footprint_bytes"] = max(
                            run["parsed"]["peak_memory_footprint_bytes"] for run in resources[implementation])
                    report["measurements"].append(item)
                    print(json.dumps(item), flush=True)
                reference = statistics.median(timings["reference"])
                candidate_time = statistics.median(timings["candidate"])
                ratio = {"mode": mode, "threads": threads,
                         "candidate_over_reference_seconds": candidate_time / reference,
                         "candidate_over_reference_throughput": reference / candidate_time}
                if args.darwin_resources:
                    ratio["candidate_over_reference_resource_medians"] = {
                        key: statistics.median(run["parsed"][key] for run in resources["candidate"])
                        / statistics.median(run["parsed"][key] for run in resources["reference"])
                        for key in DARWIN_POSITIVE_COUNTERS
                    }
                report["ratios"].append(ratio)
                print(json.dumps(ratio), flush=True)
    report["measured_parity"] = measured_parity(report["measurements"])
    report["measured_parity"]["required"] = args.require_parity
    args.results.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"measured_parity": report["measured_parity"]}), flush=True)
    return 1 if args.require_parity and not report["measured_parity"]["passed"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
