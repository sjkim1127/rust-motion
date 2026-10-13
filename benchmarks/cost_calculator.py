#!/usr/bin/env python3
"""
Cloud / Serverless Cost Battle: AWS Lambda & Fargate Cost Calculator.
Compares total infrastructure costs between Remotion and Rust Motion
for mass batch video generation (10,000 & 100,000 video workloads).
"""

import argparse
import json
import sys
from typing import Dict, Any

# AWS Lambda Pricing (us-east-1, ARM64 Graviton / x86_64)
# Source: AWS Lambda Official Pricing 2026
LAMBDA_PRICE_PER_GB_SEC_ARM = 0.0000133334  # $0.0000133334 / GB-second
LAMBDA_PRICE_PER_GB_SEC_X86 = 0.0000166667  # $0.0000166667 / GB-second
LAMBDA_REQUEST_PRICE = 0.20 / 1_000_000     # $0.20 per 1M invocations

# AWS Fargate vCPU & Memory hourly prices
FARGATE_VCPU_HOUR = 0.04048
FARGATE_GB_HOUR = 0.004445


def calculate_scenario(
    video_count: int,
    remotion_render_sec: float = 11.04,
    remotion_ram_mb: int = 3072,
    remotion_cold_start_sec: float = 4.5,
    rust_motion_render_sec: float = 2.19,
    rust_motion_ram_mb: int = 512,
    rust_motion_cold_start_sec: float = 0.1,
    arch: str = "arm64",
) -> Dict[str, Any]:
    gb_sec_rate = LAMBDA_PRICE_PER_GB_SEC_ARM if arch == "arm64" else LAMBDA_PRICE_PER_GB_SEC_X86

    # 1. Remotion calculations
    remotion_total_duration_sec = remotion_render_sec + (remotion_cold_start_sec * 0.1) # assume 10% cold start rate
    remotion_gb = remotion_ram_mb / 1024.0
    remotion_gb_seconds_per_video = remotion_gb * remotion_total_duration_sec
    remotion_total_gb_seconds = remotion_gb_seconds_per_video * video_count
    remotion_compute_cost = remotion_total_gb_seconds * gb_sec_rate
    remotion_request_cost = video_count * LAMBDA_REQUEST_PRICE
    remotion_total_cost = remotion_compute_cost + remotion_request_cost
    remotion_wallclock_hours = (remotion_total_duration_sec * video_count) / 3600.0

    # 2. Rust Motion calculations
    rust_motion_total_duration_sec = rust_motion_render_sec + (rust_motion_cold_start_sec * 0.1)
    rust_motion_gb = rust_motion_ram_mb / 1024.0
    rust_motion_gb_seconds_per_video = rust_motion_gb * rust_motion_total_duration_sec
    rust_motion_total_gb_seconds = rust_motion_gb_seconds_per_video * video_count
    rust_motion_compute_cost = rust_motion_total_gb_seconds * gb_sec_rate
    rust_motion_request_cost = video_count * LAMBDA_REQUEST_PRICE
    rust_motion_total_cost = rust_motion_compute_cost + rust_motion_request_cost
    rust_motion_wallclock_hours = (rust_motion_total_duration_sec * video_count) / 3600.0

    # Savings
    savings_dollars = remotion_total_cost - rust_motion_total_cost
    savings_percent = (savings_dollars / remotion_total_cost) * 100.0 if remotion_total_cost > 0 else 0.0
    speedup = remotion_total_duration_sec / rust_motion_total_duration_sec

    return {
        "video_count": video_count,
        "arch": arch,
        "remotion": {
            "ram_allocated_mb": remotion_ram_mb,
            "render_sec": remotion_render_sec,
            "cold_start_sec": remotion_cold_start_sec,
            "total_gb_seconds": round(remotion_total_gb_seconds, 2),
            "compute_cost_usd": round(remotion_compute_cost, 2),
            "total_cost_usd": round(remotion_total_cost, 2),
            "wallclock_hours": round(remotion_wallclock_hours, 2),
        },
        "rust-motion": {
            "ram_allocated_mb": rust_motion_ram_mb,
            "render_sec": rust_motion_render_sec,
            "cold_start_sec": rust_motion_cold_start_sec,
            "total_gb_seconds": round(rust_motion_total_gb_seconds, 2),
            "compute_cost_usd": round(rust_motion_compute_cost, 2),
            "total_cost_usd": round(rust_motion_total_cost, 2),
            "wallclock_hours": round(rust_motion_wallclock_hours, 2),
        },
        "comparison": {
            "cost_savings_usd": round(savings_dollars, 2),
            "cost_reduction_percent": round(savings_percent, 1),
            "time_speedup_factor": round(speedup, 2),
            "cost_ratio": round(remotion_total_cost / rust_motion_total_cost, 2) if rust_motion_total_cost > 0 else 0.0,
        },
    }


def format_markdown_table(res10k: Dict[str, Any], res100k: Dict[str, Any]) -> str:
    md = []
    md.append("### 💰 AWS Lambda Serverless Cost Model (Assumption-Based)\n")
    md.append("| Metric | Remotion model | **Rust Motion model** | Status |")
    md.append("|:---|:---:|:---:|:---:|")
    md.append("| **Allocated memory / render time** | configured inputs | **configured inputs** | model input, not measured infrastructure |")
    md.append(f"| **Avg Render Time** | {res10k['remotion']['render_sec']} s | **{res10k['rust-motion']['render_sec']} s** | model input |")
    md.append("| --- | --- | --- | --- |")
    md.append(f"| **10,000 Videos Cost** | **${res10k['remotion']['total_cost_usd']}** | **${res10k['rust-motion']['total_cost_usd']}** | model output only |")
    md.append(f"| **10,000 Videos Time** | {res10k['remotion']['wallclock_hours']} hrs | **{res10k['rust-motion']['wallclock_hours']} hrs** | model output only |")
    md.append("| --- | --- | --- | --- |")
    md.append(f"| **100,000 Videos Cost** | **${res100k['remotion']['total_cost_usd']}** | **${res100k['rust-motion']['total_cost_usd']}** | model output only |")
    md.append(f"| **100,000 Videos Time** | {res100k['remotion']['wallclock_hours']} hrs | **{res100k['rust-motion']['wallclock_hours']} hrs** | model output only |")
    return "\n".join(md)


def main():
    parser = argparse.ArgumentParser(description="AWS Lambda batch video rendering cost calculator.")
    parser.add_argument("--remotion-time", type=float, default=11.04, help="Remotion render time in seconds")
    parser.add_argument("--rust-motion-time", type=float, default=2.19, help="Rust Motion render time in seconds")
    parser.add_argument("--json", action="store_true", help="Output JSON instead of markdown")
    args = parser.parse_args()

    res10k = calculate_scenario(
        10_000,
        remotion_render_sec=args.remotion_time,
        rust_motion_render_sec=args.rust_motion_time,
    )
    res100k = calculate_scenario(
        100_000,
        remotion_render_sec=args.remotion_time,
        rust_motion_render_sec=args.rust_motion_time,
    )

    if args.json:
        print(json.dumps({"scenario_10k": res10k, "scenario_100k": res100k}, indent=2))
    else:
        print(format_markdown_table(res10k, res100k))


if __name__ == "__main__":
    main()
