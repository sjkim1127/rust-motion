#!/usr/bin/env python3
"""Fail when the benchmark figures quoted in the docs disagree with the committed measurement.

Each quoted comparison is derived from a committed raw file. Two checks:

1. Every figure the docs quote for a comparison must equal what its raw file gives.
2. Any table row that shows two durations and a speedup must be internally consistent, which is
   the mistake that put a 10.125 s Remotion time next to an unrelated 1.347 s native time and an
   18.66x ratio (the real ratio of those two is 7.52x).

    python3 benchmarks/check_claims.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HOSTED = "benchmarks/render-png-gha-ubuntu24.json"
M4_PRO = "benchmarks/render-png-first.json"
# Which raw files each document has to quote. BATTLE_REPORT.md is the M4 Pro machine's report.
DOCS = {
    "README.md": [HOSTED, M4_PRO],
    "benchmarks/README.md": [HOSTED, M4_PRO],
    "benchmarks/BATTLE_REPORT.md": [M4_PRO],
}
# Figures that were quoted without any raw data behind them; they must not come back.
RETRACTED = ["18.66", "18.7×", "10.125 s", "1.347 s"]

DURATION = re.compile(r"(?<![\w.])(\d+(?:\.\d+)?) s\b")
RATIO = re.compile(r"(?<![\w.])(\d+(?:\.\d+)?)\s?[x×](?!\w)")


def load_snapshot(path: str) -> tuple[float, float]:
    data = json.loads((ROOT / path).read_text(encoding="utf-8"))
    median = data.get("median_ms", {})
    native, remotion = median.get("rust-motion"), median.get("remotion")
    if not isinstance(native, (int, float)) or not isinstance(remotion, (int, float)):
        sys.exit(f"{path}: median_ms needs numeric 'rust-motion' and 'remotion' entries")
    return native / 1000, remotion / 1000


def expected_figures(path: str) -> dict[str, str]:
    native, remotion = load_snapshot(path)
    return {
        "remotion": f"{remotion:.3f} s",
        "native": f"{native:.3f} s",
        "ratio": f"{remotion / native:.2f}",
    }


def main() -> int:
    errors: list[str] = []
    texts = {doc: (ROOT / doc).read_text(encoding="utf-8") for doc in DOCS}

    for doc, snapshots in DOCS.items():
        text = texts[doc]
        for snapshot in snapshots:
            expected = expected_figures(snapshot)
            for name in ("remotion", "native"):
                if expected[name] not in text:
                    errors.append(f"{doc}: expected {name} time {expected[name]} (from {snapshot}) is not quoted")
            if not re.search(re.escape(expected["ratio"]) + r"\s?[x×]", text):
                errors.append(f"{doc}: expected speedup {expected['ratio']}x (from {snapshot}) is not quoted")
        for stale in RETRACTED:
            if stale in text:
                errors.append(f"{doc}: contains retracted figure {stale!r}")

    for doc, text in texts.items():
        for number, line in enumerate(text.splitlines(), 1):
            durations = [float(v) for v in DURATION.findall(line)]
            ratios = [float(v) for v in RATIO.findall(line)]
            if len(durations) < 2 or not ratios:
                continue
            implied = max(durations[:2]) / min(durations[:2])
            if not any(abs(ratio - implied) <= implied * 0.005 for ratio in ratios):
                errors.append(
                    f"{doc}:{number}: durations {durations[0]} s and {durations[1]} s imply "
                    f"{implied:.2f}x, but the row says {', '.join(f'{r}x' for r in ratios)}"
                )

    if errors:
        print("Benchmark claims disagree with the raw data:", file=sys.stderr)
        print("\n".join(f"  - {e}" for e in errors), file=sys.stderr)
        return 1
    for snapshot in (HOSTED, M4_PRO):
        expected = expected_figures(snapshot)
        print(f"OK: {expected['remotion']} vs {expected['native']} = {expected['ratio']}x ({snapshot})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
