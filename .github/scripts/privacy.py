#!/usr/bin/env python3
"""Check tracked/staged content and messages without displaying private values."""
import argparse
import os
from pathlib import Path
import re
import subprocess


def git(*args):
    return subprocess.check_output(["git", *args])


def violations(data, legal=False):
    if b"\0" in data:
        yield "generated binary content"
    text = data.decode("utf-8", "replace")
    folded = text.casefold()
    user = os.environ.get("USER", "") or os.environ.get("USERNAME", "")
    home = os.environ.get("HOME", "") or os.environ.get("USERPROFILE", "")
    # Hosted runner account names are infrastructure, not operator identities.
    if user and os.environ.get("GITHUB_ACTIONS") != "true" and user.casefold() in folded:
        yield "local account name"
    if home and os.environ.get("GITHUB_ACTIONS") != "true" and home.casefold() in folded:
        yield "local home directory"
    # No generic placeholder allowlist: an unrelated example never exempts a leak.
    if re.search(r"/(?:U" + r"sers|ho" + r"me)/[^/\\\s]+|[A-Za-z]:[\\/][Uu]sers[\\/][^\\/\s]+", text):
        yield "home path"
    for email in re.findall(r"[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", text):
        # Pseudonymous platform identity is permitted; no other address is exempt.
        if not legal and not email.casefold().endswith("@users.noreply.github.com"):
            yield "contact address"
    config = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
    blocked = Path(os.environ.get("GIT_PRIVACY_BLOCKLIST", os.environ.get("GIT_PRIVACY_DENYLIST", str(config / "git-privacy" / "denylist"))))
    if blocked.is_file():
        for term in blocked.read_text().splitlines():
            term = term.split("#", 1)[0].strip()
            if term and term.casefold() in folded:
                yield "machine-local blocked term"


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--staged", action="store_true")
    group.add_argument("--message", type=Path)
    args = parser.parse_args()
    if args.message:
        inputs = [("commit message", args.message.read_bytes(), False)]
    else:
        names = git("ls-files", "--cached", "-z") if args.staged else git("ls-files", "-z")
        inputs = []
        for name in names.split(b"\0"):
            if not name:
                continue
            path = os.fsdecode(name)
            data = git("show", ":" + path) if args.staged else Path(path).read_bytes()
            # Required third-party legal notices retain their original contacts.
            legal = path in {"COPYING", "LICENSE-7ZIP"}
            inputs.append((path, name + b"\n" + data, legal))
    failed = False
    for label, data, legal in inputs:
        reasons = sorted(set(violations(data, legal)))
        if reasons:
            # Do not print the potentially private filename or matched content.
            print("privacy check blocked an input: " + ", ".join(reasons))
            failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
