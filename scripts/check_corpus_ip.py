#!/usr/bin/env python3
"""CI guard: no ZBrush files in the repository unless registered as synthetic.

Rule source: docs/formats/zbrush/LEGAL.md section 5.

Fails (exit 1) if any tracked file has a ZBrush extension (.ztl .zpr .zbr
.zsc .zmt, any case) and is not listed with license_status "synthetic" in
test/corpus/zbrush/corpus.config.json (path relative to the repo root).
"""
import json
import subprocess
import sys
from pathlib import Path

EXTENSIONS = {".ztl", ".zpr", ".zbr", ".zsc", ".zmt"}
CONFIG = Path("test/corpus/zbrush/corpus.config.json")


def main() -> int:
    tracked = subprocess.run(["git", "ls-files", "-z"], check=True, capture_output=True).stdout.decode().split("\0")
    zbrush = [p for p in tracked if p and Path(p).suffix.lower() in EXTENSIONS]
    allowed = set()
    if CONFIG.exists():
        for entry in json.loads(CONFIG.read_text(encoding="utf-8")).get("files", []):
            if entry.get("license_status") == "synthetic":
                allowed.add(Path(entry.get("path", "")).as_posix())
    bad = [p for p in zbrush if Path(p).as_posix() not in allowed]
    if bad:
        print("ZBrush files committed without a 'synthetic' corpus entry (see docs/formats/zbrush/LEGAL.md §5):")
        for p in bad:
            print(f"  {p}")
        return 1
    print(f"corpus IP check passed ({len(zbrush)} registered synthetic ZBrush file(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
