#!/usr/bin/env python3
"""
Comprehensive tests for Rust Motion Python SDK bindings.
"""

import json
import math
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent.parent
TEST_OUTPUT_DIR = os.environ.get("RUST_MOTION_TEST_OUTPUT_DIR")
TARGET_DIR = Path(TEST_OUTPUT_DIR) if TEST_OUTPUT_DIR else ROOT / "target/python_test_output"
TARGET_DIR.mkdir(parents=True, exist_ok=True)

import rust_motion


def test_metadata():
    print("[*] Testing metadata and version...")
    version = rust_motion.__version__
    assert version.startswith("1.0.") or version.startswith("0.2.") or version.startswith("0.1."), f"Unexpected version: {version}"
    compositions = rust_motion.list_compositions()
    print(f"[+] Version: {version}")
    print(f"[+] Registered compositions: {compositions}")
    assert "HelloWorld" in compositions, "HelloWorld should be registered"
    print("[✓] Metadata test passed.")


def test_animation_primitives():
    print("\n[*] Testing Remotion-compatible animation primitives...")

    # 1. random()
    r1 = rust_motion.random("remotion-test")
    r2 = rust_motion.random("remotion-test")
    assert r1 == r2, f"random('remotion-test') should be deterministic: {r1} vs {r2}"
    assert 0.0 <= r1 <= 1.0, f"random() output out of [0, 1]: {r1}"

    # numeric seed
    r_num = rust_motion.random(42)
    assert 0.0 <= r_num <= 1.0

    # 2. interpolate()
    val = rust_motion.interpolate(10.0, [0.0, 20.0], [0.0, 100.0])
    assert abs(val - 50.0) < 1e-5, f"interpolate midpoint failed: {val}"

    val_clamped = rust_motion.interpolate(
        30.0, [0.0, 20.0], [0.0, 100.0], extrapolate_right="clamp"
    )
    assert abs(val_clamped - 100.0) < 1e-5

    # 3. interpolate_colors()
    # shorthand 2-color
    c1 = rust_motion.interpolate_colors("#000000", "#ffffff", 0.5)
    assert "128" in c1, f"interpolate_colors midpoint failed: {c1}"

    # Remotion multi-range
    c2 = rust_motion.interpolate_colors(
        10.0, [0.0, 10.0, 20.0], ["#000000", "#ff0000", "#ffffff"]
    )
    assert "255, 0, 0" in c2, f"interpolate_colors multi-range failed: {c2}"

    # 4. spring()
    s0 = rust_motion.spring(0.0, fps=30.0)
    s_end = rust_motion.spring(60.0, fps=30.0)
    assert abs(s0 - 0.0) < 1e-4, f"spring(0) should start near 0: {s0}"
    assert abs(s_end - 1.0) < 0.05, f"spring(60) should settle near 1: {s_end}"

    # 5. static_file()
    resolved = rust_motion.static_file("Cargo.toml")
    assert "Cargo.toml" in resolved

    print("[✓] Animation primitives test passed.")


def test_render_still():
    print("\n[*] Testing still frame render (PNG)...")
    out_png = TARGET_DIR / "test_still.png"
    if out_png.exists():
        out_png.unlink()

    rust_motion.render_still(
        composition="HelloWorld",
        frame=15,
        output=out_png,
        width=640,
        height=360,
        fps=30.0,
    )

    assert out_png.exists(), "Output PNG was not created"
    file_size = out_png.stat().st_size
    print(f"[+] Rendered still PNG size: {file_size} bytes")
    assert file_size > 1000, "Still PNG file is suspiciously small"
    print("[✓] Still frame render test passed.")


def test_render_video():
    print("\n[*] Testing video render (MP4)...")
    out_mp4 = TARGET_DIR / "test_video.mp4"
    if out_mp4.exists():
        out_mp4.unlink()

    rust_motion.render(
        composition="HelloWorld",
        output=out_mp4,
        props={"title": "Python AI Video", "subtitle": "Ultra Fast Native"},
        width=640,
        height=360,
        fps=30.0,
        duration=60, # 2 seconds
        codec="h264",
        hw_accel="disabled",
    )

    assert out_mp4.exists(), "Output MP4 was not created"
    file_size = out_mp4.stat().st_size
    print(f"[+] Rendered MP4 size: {file_size} bytes")
    assert file_size > 5000, "MP4 file is suspiciously small"

    meta = rust_motion.get_video_metadata(str(out_mp4))
    print(f"[+] Probed video metadata: {meta}")
    assert meta["width"] == 640
    assert meta["height"] == 360
    assert meta["duration_in_frames"] == 60
    assert meta["is_landscape"] is True

    print("[✓] Video render and probe test passed.")


def test_render_script():
    print("\n[*] Testing dynamic Rhai script render...")
    rhai_script = ROOT / "examples/hello.rhai"
    if not rhai_script.exists():
        print("[-] Skipping script test (examples/hello.rhai not found)")
        return

    out_rhai_mp4 = TARGET_DIR / "test_rhai.mp4"
    if out_rhai_mp4.exists():
        out_rhai_mp4.unlink()

    rust_motion.render_script(
        script_path=rhai_script,
        output=out_rhai_mp4,
        props={
            "title": "Dynamic Rhai Script",
            "subtitle": "Python AI Generation",
            "background": "#0b0d19",
            "accent": "#00f0ff88",
        },
        width=640,
        height=360,
        fps=30.0,
        duration=45,
        hw_accel="disabled",
    )

    assert out_rhai_mp4.exists(), "Output Rhai MP4 was not created"
    print(f"[+] Rendered Rhai MP4 size: {out_rhai_mp4.stat().st_size} bytes")
    print("[✓] Rhai script render test passed.")


def test_composition_class():
    print("\n[*] Testing Composition OOP class...")
    out_comp = TARGET_DIR / "test_oop.mp4"
    if out_comp.exists():
        out_comp.unlink()

    comp = rust_motion.Composition("HelloWorld", width=640, height=360, duration=30)
    print(f"[+] Initialized {comp}")
    comp.render(out_comp, props={"title": "OOP API"}, hw_accel="disabled")

    assert out_comp.exists(), "Output OOP MP4 was not created"
    print(f"[+] Rendered OOP MP4 size: {out_comp.stat().st_size} bytes")
    print("[✓] Composition OOP class test passed.")


def test_shorts_ai_video():
    print("\n[*] Testing ShortsVideo AI Builder with Whisper & Emojis...")
    whisper_json = json.dumps([
        {"word": "Awesome", "start": 0.0, "end": 0.5},
        {"word": "Shorts 🔥", "start": 0.5, "end": 1.0},
    ])
    tokens = rust_motion.parse_whisper(whisper_json)
    assert len(tokens) == 2
    assert tokens[0]["text"] == "Awesome"

    out_shorts = TARGET_DIR / "test_shorts_vertical.mp4"
    if out_shorts.exists():
        out_shorts.unlink()

    short = rust_motion.ShortsVideo(
        width=720,
        height=1280,
        fps=30.0,
        duration_in_frames=30,
        bg_color="#0b0d19",
    )
    short.add_subtitles(
        tokens,
        style=rust_motion.CaptionStyle.HORMOZI,
        active_color="#ffe600",
        font_size=48.0,
    )
    short.add_emoji("🚀", x=360.0, y=400.0, size=80.0)
    short.render(out_shorts, hw_accel="disabled")

    assert out_shorts.exists() and out_shorts.stat().st_size > 1000
    meta = rust_motion.get_video_metadata(str(out_shorts))
    assert meta["width"] == 720
    assert meta["height"] == 1280
    assert not meta["is_landscape"]
    print(f"[+] Rendered Shorts MP4 size: {out_shorts.stat().st_size} bytes ({meta['width']}x{meta['height']})")
    print("[✓] ShortsVideo AI Builder test passed.")


def test_audio_visualizer_and_ducking():
    print("\n[*] Testing Audio Visualizer & Smart Auto-Ducking Suite...")
    import wave
    import struct

    # Generate small test WAV file (440Hz sine wave)
    wav_path = TARGET_DIR / "test_sine.wav"
    sample_rate = 44100
    n_samples = int(sample_rate * 1.5) # 1.5 seconds
    with wave.open(str(wav_path), "w") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(sample_rate)
        data = bytearray()
        for i in range(n_samples):
            val = int(18000.0 * math.sin(2.0 * math.pi * 440.0 * i / sample_rate))
            data.extend(struct.pack("<h", val))
        wf.writeframes(data)

    # 1. Test spectrum calculation
    spectrum = rust_motion.get_audio_spectrum(str(wav_path), time_secs=0.5, n_bars=32)
    assert len(spectrum) == 32, "Spectrum must return 32 bins"
    assert any(b > 0.05 for b in spectrum), "Spectrum should detect sine signal"
    print(f"[+] Computed audio spectrum: max bin = {max(spectrum):.4f}")

    # 2. Test ducking keyframes calculation
    speech_ivs = [(0.5, 1.2)]
    duck_kfs = rust_motion.calculate_ducking(speech_ivs, total_duration=2.0, base_volume=0.8, duck_volume=0.15)
    assert len(duck_kfs) >= 4, "Ducking keyframes must have attack/hold/release points"
    assert duck_kfs[0][1] == 0.8, "Initial volume must be base volume"
    print(f"[+] Calculated {len(duck_kfs)} auto-ducking keyframes: {duck_kfs}")

    # 3. Test end-to-end Shorts rendering with Visualizer & Background Music
    out_audio_mp4 = TARGET_DIR / "test_audio_suite.mp4"
    if out_audio_mp4.exists():
        out_audio_mp4.unlink()

    short = rust_motion.ShortsVideo(
        width=720,
        height=1280,
        fps=30.0,
        duration_in_frames=45,
        bg_color="#050714",
    )
    short.add_voiceover(wav_path, volume=1.0)
    short.add_background_music(wav_path, volume=0.3, duck_on_voice=True, duck_volume=0.08)
    short.add_audio_visualizer(wav_path, x=100.0, y=700.0, width=520.0, height=140.0, style="bars", color="#00e5ff")
    short.add_audio_visualizer(wav_path, x=100.0, y=900.0, width=520.0, height=140.0, style="wave", color="#ff007f")
    short.render(out_audio_mp4, hw_accel="disabled")

    assert out_audio_mp4.exists() and out_audio_mp4.stat().st_size > 5000
    print(f"[+] Rendered Audio Suite Shorts MP4: {out_audio_mp4.stat().st_size} bytes")
    print("[✓] Audio Visualizer & Smart Auto-Ducking Suite test passed.")


def test_remotion_paths_shapes_and_layout():
    print("\n[*] Testing Remotion Paths, Shapes, Layout & Typography Parity...")

    # 1. Bounding box & Path transformations
    path = "M 10 20 L 110 20 L 110 70 L 10 70 Z"
    bbox = rust_motion.get_bounding_box(path)
    assert bbox is not None, "Bounding box must be computed"
    assert bbox["x"] == 10.0 and bbox["y"] == 20.0
    assert bbox["width"] == 100.0 and bbox["height"] == 50.0
    print(f"[+] Computed path bbox: {bbox}")

    rev = rust_motion.reverse_path("M 0 0 L 10 10 L 20 20")
    assert "M 20.0000 20.0000" in rev
    print(f"[+] Reversed path: {rev}")

    rot = rust_motion.rotate_path("M 10 0 L 20 0", math.pi / 2, 0.0, 0.0)
    assert "M 0.0000 10.0000" in rot
    print(f"[+] Rotated path: {rot}")

    tangent = rust_motion.get_tangent_at_length("M 0 0 L 100 0", 50.0)
    assert tangent is not None and abs(tangent[0] - 1.0) < 1e-4 and abs(tangent[1]) < 1e-4
    print(f"[+] Path tangent vector at length 50: {tangent}")

    # 2. Shape generators
    ellipse = rust_motion.make_ellipse(80.0, 40.0)
    assert "M 80 0" in ellipse and "A 80 40" in ellipse
    print(f"[+] Generated Ellipse SVG: {ellipse[:40]}...")

    # 3. Safe area & Layout
    safe_tiktok = rust_motion.get_safe_area_insets("tiktok", 1080.0, 1920.0)
    assert safe_tiktok["top"] == 120.0 and safe_tiktok["bottom"] == 340.0
    assert safe_tiktok["safe_width"] == 1080.0 - 40.0 - 120.0
    print(f"[+] TikTok Safe Area: {safe_tiktok}")

    short = rust_motion.ShortsVideo(width=720, height=1280)
    short_safe = short.get_safe_area("reels")
    assert short_safe["top"] > 50.0
    print(f"[+] ShortsVideo.get_safe_area('reels'): {short_safe}")

    optimal_font_size = rust_motion.fit_text("BIG VIRAL HEADLINE", max_width=400.0, max_height=80.0)
    assert 12.0 <= optimal_font_size <= 120.0
    print(f"[+] fit_text optimal font size: {optimal_font_size:.2f}px")

    # 4. Kinetic Typography
    typed = rust_motion.typewriter_text("Hello World", progress=0.5, show_cursor=True)
    assert typed.startswith("Hello") and typed.endswith("|")
    print(f"[+] Typewriter progress 0.5: '{typed}'")

    scrambled = rust_motion.scramble_text("TOP SECRET", progress=0.5, seed=123)
    assert scrambled.startswith("TOP S")
    assert len(scrambled) == len("TOP SECRET")
    print(f"[+] Scrambled text progress 0.5: '{scrambled}'")

    print("[✓] Remotion Paths, Shapes, Layout & Typography test passed.")


def main():
    print("=" * 60)
    print("🚀 Running Rust Motion Python SDK Test Suite")
    print("=" * 60)
    test_metadata()
    test_animation_primitives()
    test_render_still()
    test_render_video()
    test_render_script()
    test_composition_class()
    test_shorts_ai_video()
    test_audio_visualizer_and_ducking()
    test_remotion_paths_shapes_and_layout()
    print("\n" + "=" * 60)
    print("🎉 ALL PYTHON SDK TESTS PASSED PERFECTLY!")
    print("=" * 60)


if __name__ == "__main__":
    main()
