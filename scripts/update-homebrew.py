#!/usr/bin/env python3
"""Render the Homebrew formula from a published release's checksum manifest."""
import argparse
import re
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--tag", required=True)
parser.add_argument("--checksums", type=Path, required=True)
args = parser.parse_args()
if not re.fullmatch(r"v\d+\.\d+\.\d+", args.tag):
    parser.error("tag must be a stable version such as v0.1.0")

checksums = {}
for line in args.checksums.read_text().splitlines():
    fields = line.split()
    if len(fields) != 2 or not re.fullmatch(r"[0-9a-fA-F]{64}", fields[0]):
        parser.error("invalid SHA256SUMS entry")
    name = fields[1].removeprefix("*")
    if name in checksums:
        parser.error("duplicate SHA256SUMS entry")
    checksums[name] = fields[0].lower()

root = Path(__file__).resolve().parent.parent
text = (root / "scripts/aigc.rb.in").read_text()
for token, value in {
    "@VERSION@": args.tag[1:],
    "@ARM_SHA256@": checksums.get("aigc-aarch64-apple-darwin.tar.gz"),
    "@INTEL_SHA256@": checksums.get("aigc-x86_64-apple-darwin.tar.gz"),
}.items():
    if value is None:
        parser.error("checksum manifest must include both Mac architectures")
    text = text.replace(token, value)
(root / "Formula").mkdir(exist_ok=True)
(root / "Formula/aigc.rb").write_text(text)
print(f"Updated Formula/aigc.rb for {args.tag}")
