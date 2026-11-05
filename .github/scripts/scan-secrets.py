#!/usr/bin/env python3
"""Scan complete Git index blobs, including unchanged files and unborn indexes."""
import argparse
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--docker", action="store_true")
    args = parser.parse_args()
    root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
    names = subprocess.check_output(["git", "ls-files", "--cached", "-z"])
    # Disk-backed aggregation avoids pipe deadlocks and unbounded aggregate RAM.
    # The scanner receives complete blobs, never a diff or a working-tree copy.
    with tempfile.TemporaryFile() as aggregate:
        for name in dict.fromkeys(names.split(b"\0")):
            if not name:
                continue
            aggregate.write(b"\n")
            aggregate.write(name)
            aggregate.write(b"\n")
            subprocess.run([b"git", b"show", b":" + name], stdout=aggregate, check=True)
        aggregate.seek(0)
        if args.docker:
            command = ["docker", "run", "--rm", "-i", "-v", str(root) + ":/repo:ro",
                       "zricethezav/gitleaks:v8.30.1", "stdin", "--redact", "--config=/repo/.gitleaks.toml"]
        else:
            command = ["gitleaks", "stdin", "--redact", "--config=" + str(root / ".gitleaks.toml")]
        return subprocess.run(command, stdin=aggregate).returncode


if __name__ == "__main__":
    raise SystemExit(main())
