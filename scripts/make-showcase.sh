#!/usr/bin/env bash
# Regenerate the README showcase assets (assets/showcase.mp4 and assets/showcase.gif)
# from examples/showcase.rhai, so the banner always matches the current renderer and
# project name.
#
# Usage: scripts/make-showcase.sh [path/to/rust-motion]
# Requires: ffmpeg on PATH.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${1:-${RUST_MOTION_BIN:-$ROOT/target/release/rust-motion}}"
OUT="$ROOT/assets"

if [ ! -x "$BIN" ]; then
  echo "rust-motion binary not found at $BIN" >&2
  echo "build it with: cargo build --release -p rust-motion-cli" >&2
  exit 1
fi
command -v ffmpeg >/dev/null || { echo "ffmpeg is required" >&2; exit 1; }

# 5 seconds, 1080p at 60 fps. The scene loops seamlessly (see examples/showcase.rhai).
"$BIN" render --script "$ROOT/examples/showcase.rhai" \
  --width 1920 --height 1080 --fps 60 --duration 300 \
  --codec h264 --output "$OUT/showcase.mp4" >/dev/null

# 960x540 at 25 fps with a per-clip palette for a small, clean GIF.
ffmpeg -v error -y -i "$OUT/showcase.mp4" \
  -vf "fps=25,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
  -loop 0 "$OUT/showcase.gif"

ls -lh "$OUT/showcase.mp4" "$OUT/showcase.gif"
