#!/usr/bin/env sh
set -eu

IMAGE=${IMAGE:-rust-motion:latest}
PLATFORM=${PLATFORM:-linux/arm64}
OUT_DIR=${OUT_DIR:-target/container-smoke}
OUTPUT="$OUT_DIR/hello-world.mp4"

mkdir -p "$OUT_DIR"

docker run --rm --platform "$PLATFORM" \
  -v "$(pwd)/$OUT_DIR:/work/out" \
  "$IMAGE" render \
  --composition HelloWorld \
  --output /work/out/hello-world.mp4 \
  --width 320 --height 180 --fps 30 --duration 16 \
  --codec h264 --backend native --timeout-seconds 60

ffprobe -v error \
  -select_streams v:0 \
  -show_entries stream=codec_name,width,height,nb_frames \
  -show_entries format=duration,size \
  -of default=noprint_wrappers=1 "$OUTPUT"

test -s "$OUTPUT"
echo "container smoke passed: $OUTPUT"
