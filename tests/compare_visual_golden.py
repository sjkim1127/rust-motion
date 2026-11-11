#!/usr/bin/env python3
"""
Compares rendered test snapshots against committed golden reference frames.
Calculates both MAE (Mean Absolute Error) and SSIM (Structural Similarity Index).
Exits with code 0 if all frames are within tolerance (MAE <= 0.5% AND SSIM >= 0.99), or code 1 on regression.
"""

import argparse
import sys
from pathlib import Path

try:
    from PIL import Image
    import numpy as np
except ImportError:
    print("::error::PIL (Pillow) and numpy are required to run visual golden comparison.")
    print("Run: pip install pillow numpy")
    sys.exit(1)


def compute_ssim_numpy(im1: np.ndarray, im2: np.ndarray) -> float:
    """Computes Structural Similarity Index (SSIM) between two RGB/RGBA images using pure NumPy."""
    # Convert RGB/RGBA to luminance Y = 0.299 R + 0.587 G + 0.114 B
    y1 = 0.299 * im1[:, :, 0] + 0.587 * im1[:, :, 1] + 0.114 * im1[:, :, 2]
    y2 = 0.299 * im2[:, :, 0] + 0.587 * im2[:, :, 1] + 0.114 * im2[:, :, 2]

    k1, k2, L = 0.01, 0.03, 255.0
    c1 = (k1 * L) ** 2
    c2 = (k2 * L) ** 2

    # 11-tap 1D Gaussian kernel (sigma = 1.5)
    kernel = np.array(
        [0.0010, 0.0076, 0.0360, 0.1094, 0.2130, 0.2660, 0.2130, 0.1094, 0.0360, 0.0076, 0.0010],
        dtype=np.float32,
    )
    kernel /= kernel.sum()

    def filter2d(img):
        # Separable 1D convolution along horizontal then vertical axis
        r = np.apply_along_axis(lambda x: np.convolve(x, kernel, mode="valid"), 1, img)
        return np.apply_along_axis(lambda x: np.convolve(x, kernel, mode="valid"), 0, r)

    mu1 = filter2d(y1)
    mu2 = filter2d(y2)
    mu1_sq = mu1 * mu1
    mu2_sq = mu2 * mu2
    mu1_mu2 = mu1 * mu2

    sigma1_sq = filter2d(y1 * y1) - mu1_sq
    sigma2_sq = filter2d(y2 * y2) - mu2_sq
    sigma12 = filter2d(y1 * y2) - mu1_mu2

    numerator = (2 * mu1_mu2 + c1) * (2 * sigma12 + c2)
    denominator = (mu1_sq + mu2_sq + c1) * (sigma1_sq + sigma2_sq + c2)
    ssim_map = numerator / denominator
    return float(np.mean(ssim_map))


def compare_images(
    actual_path: Path,
    golden_path: Path,
    max_mae: float = 0.005,
    min_ssim: float = 0.99,
) -> tuple[bool, float, float, str]:
    if not actual_path.is_file():
        return False, 1.0, 0.0, f"Actual snapshot missing: {actual_path}"
    if not golden_path.is_file():
        return False, 1.0, 0.0, f"Golden reference missing: {golden_path}"

    with Image.open(actual_path) as img_a, Image.open(golden_path) as img_g:
        if img_a.size != img_g.size:
            return False, 1.0, 0.0, f"Dimension mismatch: actual {img_a.size} vs golden {img_g.size}"

        arr_a = np.asarray(img_a.convert("RGBA"), dtype=np.float32)
        arr_g = np.asarray(img_g.convert("RGBA"), dtype=np.float32)

        abs_diff = np.abs(arr_a - arr_g)
        mae = float(np.mean(abs_diff) / 255.0)

        # Compute SSIM
        ssim = compute_ssim_numpy(arr_a, arr_g)

        passed = (mae <= max_mae) and (ssim >= min_ssim)
        details = f"MAE: {mae * 100.0:.4f}% | SSIM: {ssim:.4f}"
        return passed, mae, ssim, details


def main():
    parser = argparse.ArgumentParser(description="Compare visual snapshots against golden references")
    parser.add_argument("--actual-dir", type=Path, default=Path("snapshots"), help="Directory with actual rendered PNGs")
    parser.add_argument("--golden-dir", type=Path, default=Path("tests/visual_golden"), help="Directory with golden reference PNGs")
    parser.add_argument("--max-mae", type=float, default=0.005, help="Maximum allowed Mean Absolute Error (default 0.005 = 0.5%%)")
    parser.add_argument("--min-ssim", type=float, default=0.99, help="Minimum allowed SSIM index (default 0.99)")
    args = parser.parse_args()

    golden_files = sorted(args.golden_dir.glob("*.png"))
    if not golden_files:
        print(f"::error::No golden images found in {args.golden_dir}")
        sys.exit(1)

    print(f"🎬 Comparing {len(golden_files)} snapshots against golden references...")
    all_passed = True
    results = []

    for golden_path in golden_files:
        filename = golden_path.name
        actual_path = args.actual_dir / filename
        passed, mae, ssim, details = compare_images(actual_path, golden_path, args.max_mae, args.min_ssim)

        status_icon = "✅" if passed else "❌"
        results.append((status_icon, filename, f"{mae * 100.0:.4f}%", f"{ssim:.4f}", "PASS" if passed else "FAIL"))
        if not passed:
            all_passed = False

    print("\n| Status | Frame Snapshot | MAE | SSIM | Result |")
    print("|:------:|:---------------|:----|:-----|:------:|")
    for status, name, mae_str, ssim_str, res in results:
        print(f"| {status} | `{name}` | {mae_str} | {ssim_str} | **{res}** |")

    if all_passed:
        print("\n🎉 All visual regression tests passed within tolerance!")
        sys.exit(0)
    else:
        print("\n❌ Visual regression detected! One or more frames exceeded difference tolerances.")
        sys.exit(1)


if __name__ == "__main__":
    main()
