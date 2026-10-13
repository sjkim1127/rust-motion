#!/usr/bin/env python3
"""
Hardware Acceleration Benchmark: Apple VideoToolbox vs CPU libx264 / libx265.
Compares encode throughput (FPS), total render time, and file size across resolutions.
"""

import time
import os
import json
import subprocess
from pathlib import Path
import rust_motion

def get_codec_info(file_path):
    try:
        cmd = [
            "ffprobe", "-v", "error", "-select_streams", "v:0",
            "-show_entries", "stream=codec_name,width,height,bit_rate",
            "-of", "json", str(file_path)
        ]
        out = subprocess.check_output(cmd, stderr=subprocess.DEVNULL)
        info = json.loads(out)
        stream = info.get("streams", [{}])[0]
        return stream.get("codec_name", "unknown")
    except Exception:
        return "unknown"

def run_test(name, fn, output_path, total_frames):
    if os.path.exists(output_path):
        os.remove(output_path)
    
    start_time = time.perf_counter()
    fn()
    elapsed = time.perf_counter() - start_time
    
    fps = total_frames / elapsed if elapsed > 0 else 0
    size_mb = os.path.getsize(output_path) / (1024 * 1024) if os.path.exists(output_path) else 0
    actual_codec = get_codec_info(output_path)
    
    print(f"| {name:<35} | {elapsed:>6.3f}s | {fps:>7.1f} FPS | {size_mb:>6.2f} MB | {actual_codec:<10} |")
    return {
        "name": name,
        "elapsed": elapsed,
        "fps": fps,
        "size_mb": size_mb,
        "codec": actual_codec
    }

def main():
    print("=" * 85)
    print("🔥 Rust Motion Hardware Acceleration (VideoToolbox vs Software CPU) Benchmark")
    print("=" * 85)
    print()

    out_dir = Path("/tmp/rust_motion_bench_hw")
    out_dir.mkdir(parents=True, exist_ok=True)

    # 1. 1080p 60 frames (HelloWorld)
    print("### Test 1: 1080p (1920x1080, 60 frames, HelloWorld)")
    print("-" * 85)
    print(f"| {'Configuration':<35} | {'Time':>7} | {'Speed':>11} | {'Size':>9} | {'FFmpeg Codec':<10} |")
    print("-" * 85)

    sw_1080 = run_test(
        "1080p H.264 Software (libx264)",
        lambda: rust_motion.render(
            "HelloWorld",
            out_dir / "sw_1080.mp4",
            width=1920,
            height=1080,
            fps=30.0,
            duration=60,
            codec="h264",
            hw_accel="disabled"
        ),
        out_dir / "sw_1080.mp4",
        60
    )

    hw_1080 = run_test(
        "1080p H.264 VideoToolbox (Apple HW)",
        lambda: rust_motion.render(
            "HelloWorld",
            out_dir / "hw_1080.mp4",
            width=1920,
            height=1080,
            fps=30.0,
            duration=60,
            codec="h264",
            hw_accel="videotoolbox"
        ),
        out_dir / "hw_1080.mp4",
        60
    )

    auto_1080 = run_test(
        "1080p H.264 Auto (Auto -> VT)",
        lambda: rust_motion.render(
            "HelloWorld",
            out_dir / "auto_1080.mp4",
            width=1920,
            height=1080,
            fps=30.0,
            duration=60,
            codec="h264",
            hw_accel="auto"
        ),
        out_dir / "auto_1080.mp4",
        60
    )
    print("-" * 85)
    print()

    # 2. 4K Cinematic Scene (3840x2160, 60 frames, Rhai Script)
    script_path = Path("examples/templates/cinematic_4k.rhai")
    if script_path.exists():
        print("### Test 2: 4K Ultra HD (3840x2160, 60 frames, Cinematic Motion Graphics)")
        print("-" * 85)
        print(f"| {'Configuration':<35} | {'Time':>7} | {'Speed':>11} | {'Size':>9} | {'FFmpeg Codec':<10} |")
        print("-" * 85)

        sw_4k = run_test(
            "4K H.264 Software (libx264)",
            lambda: rust_motion.render_script(
                script_path,
                out_dir / "sw_4k.mp4",
                width=3840,
                height=2160,
                fps=60.0,
                duration=60,
                codec="h264",
                hw_accel="disabled"
            ),
            out_dir / "sw_4k.mp4",
            60
        )

        hw_4k = run_test(
            "4K H.264 VideoToolbox (Apple HW)",
            lambda: rust_motion.render_script(
                script_path,
                out_dir / "hw_4k.mp4",
                width=3840,
                height=2160,
                fps=60.0,
                duration=60,
                codec="h264",
                hw_accel="videotoolbox"
            ),
            out_dir / "hw_4k.mp4",
            60
        )

        # 3. 4K HEVC / H.265 Test (HEVC software is notorious for high CPU load)
        sw_4k_hevc = run_test(
            "4K HEVC Software (libx265)",
            lambda: rust_motion.render_script(
                script_path,
                out_dir / "sw_4k_hevc.mp4",
                width=3840,
                height=2160,
                fps=60.0,
                duration=60,
                codec="h265",
                hw_accel="disabled"
            ),
            out_dir / "sw_4k_hevc.mp4",
            60
        )

        hw_4k_hevc = run_test(
            "4K HEVC VideoToolbox (Apple HW)",
            lambda: rust_motion.render_script(
                script_path,
                out_dir / "hw_4k_hevc.mp4",
                width=3840,
                height=2160,
                fps=60.0,
                duration=60,
                codec="h265",
                hw_accel="videotoolbox"
            ),
            out_dir / "hw_4k_hevc.mp4",
            60
        )

        print("-" * 85)
        print()

        h264_speedup = sw_4k["elapsed"] / hw_4k["elapsed"] if hw_4k["elapsed"] > 0 else 1.0
        hevc_speedup = sw_4k_hevc["elapsed"] / hw_4k_hevc["elapsed"] if hw_4k_hevc["elapsed"] > 0 else 1.0

        print("⚡ Speedup Highlights:")
        print(f"  • 4K H.264 Hardware Acceleration: {h264_speedup:.2f}x faster ({hw_4k['fps']:.1f} FPS vs {sw_4k['fps']:.1f} FPS)")
        print(f"  • 4K HEVC Hardware Acceleration:  {hevc_speedup:.2f}x faster ({hw_4k_hevc['fps']:.1f} FPS vs {sw_4k_hevc['fps']:.1f} FPS)")
        print()

    print("✅ All hardware acceleration benchmarks completed successfully!")

if __name__ == "__main__":
    main()
