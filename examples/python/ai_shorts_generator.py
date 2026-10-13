#!/usr/bin/env python3
"""
AI Shorts & Reels Generator with Rust Motion Python SDK.
Generates vertical 9:16 (1080x1920) videos with zero browser overhead in ~1-2 seconds.
"""

import argparse
import time
from pathlib import Path
import rust_motion

ROOT = Path(__file__).resolve().parent.parent.parent
TEMPLATE_PATH = ROOT / "examples/templates/shorts_caption.rhai"
OUTPUT_DIR = ROOT / "target/generated_shorts"
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)


def generate_short(
    headline: str,
    caption: str,
    tag: str = "TECH PULSE",
    bg_color: str = "#0b0d19",
    accent_color: str = "#ff007f33",
    output_filename: str = "short_video.mp4",
    duration_sec: float = 4.0,
    fps: float = 30.0,
) -> Path:
    output_path = OUTPUT_DIR / output_filename
    total_frames = int(duration_sec * fps)

    props = {
        "headline": headline,
        "caption": caption,
        "tag": tag,
        "background": bg_color,
        "accent": accent_color,
        "text_color": "#ffffff",
    }

    print(f"[*] Generating 9:16 vertical short ({total_frames} frames @ {fps}fps)...")
    print(f"    Headline: '{headline}'")
    print(f"    Tag: [{tag}]")

    start_time = time.time()

    rust_motion.render_script(
        script_path=TEMPLATE_PATH,
        output=output_path,
        props=props,
        width=1080,
        height=1920,
        fps=fps,
        duration=total_frames,
        codec="h264",
        crf=18,
        preset="fast",
    )

    elapsed = time.time() - start_time
    file_size_mb = output_path.stat().st_size / (1024 * 1024)

    print(f"\n[+] Render completed in {elapsed:.2f} seconds!")
    print(f"[+] Output: {output_path} ({file_size_mb:.2f} MB)")
    print(f"[+] Effective speed: {total_frames / elapsed:.1f} FPS ({(duration_sec / elapsed):.1f}x Real-time)")

    return output_path


def main():
    parser = argparse.ArgumentParser(description="Generate vertical 9:16 AI Shorts.")
    parser.add_argument("--headline", default="Python meets Rust:\nZero-Browser Video Revolution", help="Main title")
    parser.add_argument("--caption", default="Generated natively in 1.5 seconds without Chrome, Node, or Puppeteer.", help="Subtitle caption")
    parser.add_argument("--tag", default="GEN-AI VIDEO", help="Category tag badge")
    parser.add_argument("--output", default="ai_shorts_demo.mp4", help="Output file name")
    parser.add_argument("--duration", type=float, default=3.0, help="Duration in seconds (default: 3.0)")
    args = parser.parse_args()

    generate_short(
        headline=args.headline,
        caption=args.caption,
        tag=args.tag,
        output_filename=args.output,
        duration_sec=args.duration,
    )


if __name__ == "__main__":
    main()
