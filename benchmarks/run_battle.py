#!/usr/bin/env python3
"""
Master Orchestrator for the Remotion vs Rust Motion 3-Axis Battle Benchmark.
Generates comprehensive results and writes BATTLE_REPORT.md.
"""

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BENCH_DIR = ROOT / "benchmarks"
TARGET_DIR = ROOT / "target"


def run_cmd(cmd, cwd=ROOT):
    print(f"[*] Running: {' '.join(cmd)}")
    res = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)
    if res.returncode != 0:
        print(f"[!] Error running {' '.join(cmd)}:\n{res.stderr}", file=sys.stderr)
    return res


def measured_render_baseline():
    """Return the paired native/Remotion timing report when one exists."""
    report_path = BENCH_DIR / "render-png.json"
    if not report_path.exists():
        return None
    with report_path.open(encoding="utf-8") as f:
        report = json.load(f)
    medians = report.get("median_ms", {})
    native_ms = medians.get("rust-motion")
    remotion_ms = medians.get("remotion")
    if not isinstance(native_ms, (int, float)) or not isinstance(remotion_ms, (int, float)):
        return None
    return {"native_sec": native_ms / 1000, "remotion_sec": remotion_ms / 1000}


def main():
    parser = argparse.ArgumentParser(description="Remotion vs Rust Motion 3-Axis Benchmark Battle")
    parser.add_argument("--skip-build", action="store_true", help="Skip cargo build")
    parser.add_argument("--quick", action="store_true", help="Quick mode (single run)")
    args = parser.parse_args()

    print("\n" + "="*70)
    print("⚔️   REMOTION VS RUST_MOTION: THE 3-AXIS BENCHMARK BATTLE   ⚔️")
    print("="*70 + "\n")

    # 1. Build release binaries
    if not args.skip_build:
        print("[*] Ensuring release binaries are built...")
        subprocess.run([
            "cargo", "build", "--release",
            "-p", "rust-motion-core",
            "--example", "render_bench",
            "--example", "cyberpunk_bench"
        ], cwd=ROOT, check=True)
        print("[+] Release binaries ready.\n")

    # 2. Run Memory & 720p Render Battle (using profile_memory.py)
    print("--- [ROUND 1: Memory Footprint (RSS) & 720p Render Speed] ---")
    native_bench_bin = ROOT / "target/release/examples/render_bench"
    output_mp4 = TARGET_DIR / "native_bench.mp4"
    native_mem_json = BENCH_DIR / "memory_rust_motion_720p.json"

    subprocess.run([
        sys.executable, str(BENCH_DIR / "profile_memory.py"),
        "-o", str(native_mem_json),
        "--", str(native_bench_bin), str(output_mp4)
    ], check=True)

    with open(native_mem_json, "r") as f:
        rust_motion_720p_mem = json.load(f)

    # Load previous 720p Remotion render results if available
    remotion_render_file = BENCH_DIR / "render-png-first.json"
    remotion_720p_data = None
    if remotion_render_file.exists():
        with open(remotion_render_file, "r") as f:
            remotion_720p_data = json.load(f)

    print("\n--- [ROUND 2: 1080p Cyberpunk VFX & Text Auto-Fitting Battle] ---")
    cyberpunk_bin = ROOT / "target/release/examples/cyberpunk_bench"
    cyberpunk_mp4 = TARGET_DIR / "rust_motion_cyberpunk.mp4"
    cyberpunk_mem_json = BENCH_DIR / "memory_rust_motion_cyberpunk.json"

    subprocess.run([
        sys.executable, str(BENCH_DIR / "profile_memory.py"),
        "-o", str(cyberpunk_mem_json),
        "--", str(cyberpunk_bin), str(cyberpunk_mp4), "4"
    ], check=True)

    with open(cyberpunk_mem_json, "r") as f:
        rust_motion_cyberpunk_mem = json.load(f)

    print("\n--- [ROUND 3: Cloud / Serverless Cost & Container Size Battle] ---")
    cost_res = subprocess.run([
        sys.executable, str(BENCH_DIR / "cost_calculator.py"), "--json"
    ], capture_output=True, text=True, check=True)
    cost_data = json.loads(cost_res.stdout)

    # 4. Generate BATTLE_REPORT.md
    report_file = BENCH_DIR / "BATTLE_REPORT.md"
    print(f"\n[*] Generating Battle Report: {report_file}...")

    scenario_10k = cost_data["scenario_10k"]
    scenario_100k = cost_data["scenario_100k"]

    d_720p_peak = rust_motion_720p_mem["peak_rss_mb"]
    d_720p_time = rust_motion_720p_mem["duration_sec"]
    d_cyber_peak = rust_motion_cyberpunk_mem["peak_rss_mb"]
    d_cyber_time = rust_motion_cyberpunk_mem["duration_sec"]
    paired_render = measured_render_baseline()
    paired_remotion_time = (
        f"{paired_render['remotion_sec']:.3f} s" if paired_render else "not measured"
    )
    paired_speedup = (
        f"{paired_render['remotion_sec'] / paired_render['native_sec']:.2f}x"
        if paired_render
        else "not available"
    )
    remotion_memory_note = "not measured"
    remotion_concurrency_note = "not measured"

    report_content = f"""# ⚔️ Remotion vs Rust Motion: 3-Axis Benchmark Battle Report

**Generated**: {time.strftime('%Y-%m-%d %H:%M:%S')}
**Platform**: {rust_motion_720p_mem.get('platform', 'macOS')} ({rust_motion_cyberpunk_mem.get('cpu', 'Apple Silicon')})

---

## 🥊 Round 1: Memory Footprint (Peak RSS) & Server Concurrency

Remotion requires Node.js and Chromium renderer/GPU child processes, while Rust Motion operates as a single multithreaded native binary with zero browser overhead.

| Workload | Remotion (Chromium 4 Workers) | **Rust Motion (tiny-skia 4 Threads)** | Difference / Advantage |
|:---|:---:|:---:|:---:|
| **720p Spring Scene Peak RAM** | {remotion_memory_note} | **{d_720p_peak} MB** | **no memory ratio claim** |
| **1080p Cyberpunk VFX Peak RAM**| {remotion_memory_note} | **{d_cyber_peak} MB** | **no memory ratio claim** |
| **720p Spring Scene Render Duration** | {paired_remotion_time} | **{d_720p_time} s** | **{paired_speedup} paired speedup when measured** |
| **1080p VFX Render Duration** | not measured by this harness | **{d_cyber_time} s** | **no speedup claim** |

### 🖥️ Concurrency Capacity on Common Cloud Servers (Available RAM)

How many video renders can run concurrently on a single standard server before crashing out of memory (OOM)?

| Server Specs | Remotion Concurrency | **Rust Motion Concurrency** | Scale Multiplier |
|:---|:---:|:---:|:---:|
| **4GB RAM Server** (Usable: 3GB) | {remotion_concurrency_note} | **{rust_motion_720p_mem['concurrency_capacity']['server_4gb_ram']} concurrent renders** | **not comparable** |
| **8GB RAM Server** (Usable: 7GB) | {remotion_concurrency_note} | **{rust_motion_720p_mem['concurrency_capacity']['server_8gb_ram']} concurrent renders** | **not comparable** |
| **16GB RAM Server** (Usable: 15GB) | {remotion_concurrency_note} | **{rust_motion_720p_mem['concurrency_capacity']['server_16gb_ram']} concurrent renders** | **not comparable** |

---

## 🎨 Round 2: Text Auto-Fitting & Complex VFX (1080p Cyberpunk Title)

Features exercised:
- Multi-line text box layout with binary-search font size auto-fitting (`fit_text_on_n_lines` / `layout_text_box`)
- Neon magenta offscreen drop-shadow blur glow (`SceneShadow`)
- Chromatic aberration color fringe filter (`SceneFilter::ChromaticAberration`)
- Fullscreen vignette edge falloff (`SceneFilter::Vignette`)
- Animated procedural neon laser grid

**Rust Motion 1080p Result**:
- Duration: **{d_cyber_time}s** (180 frames @ 30fps)
- Average Throughput: **{round(180 / max(0.1, d_cyber_time), 1)} FPS**
- Peak Memory: **{d_cyber_peak} MB** (Completely immune to browser GC spikes)

---

## 💰 Round 3: Serverless / Cloud Cost Model (Assumption-Based)

Calculated using official AWS Lambda pricing ($0.0000133334/GB-s on ARM64 Graviton) across batch production runs:

| Metric | Remotion model | **Rust Motion model** | Status |
|:---|:---:|:---:|:---:|
| **Allocated memory / render time** | configured inputs | **configured inputs** | model input, not measured infrastructure |
| **10,000 Videos Cost** | **${scenario_10k['remotion']['total_cost_usd']}** | **${scenario_10k['rust-motion']['total_cost_usd']}** | model output only |
| **10,000 Videos Wallclock** | {scenario_10k['remotion']['wallclock_hours']} hrs | **{scenario_10k['rust-motion']['wallclock_hours']} hrs** | model output only |
| **100,000 Videos Cost** | **${scenario_100k['remotion']['total_cost_usd']}** | **${scenario_100k['rust-motion']['total_cost_usd']}** | model output only |
| **100,000 Videos Wallclock** | {scenario_100k['remotion']['wallclock_hours']} hrs | **{scenario_100k['rust-motion']['wallclock_hours']} hrs** | model output only |

---

## 🏆 Final Verdict

1. **Efficiency**: Rust Motion's native RSS is measured here; Remotion RSS requires a matching profile before a memory multiplier can be claimed.
2. **Speed**: only paired workloads with recorded timings receive a speedup claim; the 1080p VFX result is reported without an unsupported Remotion comparison.
3. **Economics**: cost figures remain model assumptions from `cost_calculator.py`; container size, cold start, and Remotion infrastructure RSS are not measured by this harness.
"""

    with open(report_file, "w", encoding="utf-8") as f:
        f.write(report_content)

    print(f"\n[+] Battle completed successfully! Report saved to {report_file}\n")
    print(report_content)


if __name__ == "__main__":
    main()
