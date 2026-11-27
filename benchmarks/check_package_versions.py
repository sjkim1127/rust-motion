#!/usr/bin/env python3
"""Fail when packages of the Remotion benchmark that must match are on different versions.

Remotion refuses to render a bundle built by another version of itself, and React refuses to run
with a different react-dom, so a partial dependency update leaves the benchmark unable to run.

    python3 benchmarks/check_package_versions.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

MANIFEST = Path(__file__).resolve().parent / "remotion" / "package.json"
FAMILIES = {
    "Remotion": lambda name: name == "remotion" or name.startswith("@remotion/"),
    "React": lambda name: name in ("react", "react-dom"),
}


def main() -> int:
    dependencies = json.loads(MANIFEST.read_text(encoding="utf-8"))["dependencies"]
    failed = False
    for family, belongs in FAMILIES.items():
        versions = {name: version for name, version in dependencies.items() if belongs(name)}
        if len(set(versions.values())) != 1:
            listed = ", ".join(f"{name} {version}" for name, version in sorted(versions.items()))
            print(f"{family} packages must share one version: {listed or 'none found'}", file=sys.stderr)
            failed = True
        else:
            print(f"OK: {family} {next(iter(versions.values()))} ({len(versions)} packages)")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
