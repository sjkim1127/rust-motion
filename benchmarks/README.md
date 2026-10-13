# Performance acceptance

Rust Motion should beat Remotion on the corresponding workload while preserving
output. These benchmarks run the real Remotion implementation with its caches
enabled. Each script saves raw samples and exits unsuccessfully if output checks
fail or the measured median does not beat Remotion. Performance gates belong on
an otherwise idle machine; they are not timing assertions in ordinary unit tests.

## Spring kernel

From the repository root, with Node 24 and Rust installed:

```sh
python3 benchmarks/compare-spring.py --output benchmarks/spring-after.json
```

This builds the Rust example in release mode and imports the TypeScript from
`vendor/remotion-4.0.495/packages/core/src/spring`. It runs five fresh processes
per engine/workload, alternating engine order. The cold case evaluates 300
frames without warmup. Other cases warm 10,000 calls, then time 100,000 calls.
The timer excludes process startup, module loading, and compilation on both
sides. Checksums must agree within 1e-6 before a speedup is reported.

Workloads cover a repeated frame, sequential playback, an explicit duration,
fractional random seeks, settling measurements, and 32 interleaved physics
configurations. Remotion's advance, calculation, and measurement caches remain
active, including during timed loops. `spring-before.json` records the working
tree before this optimization; rerunning the current code produces the optimized
results, not that historical baseline.

The optimization evaluates the oscillator at the equivalent elapsed time in
constant time, preserving Remotion's 64ms step cap and final fractional step.
Settling durations use a 64-entry cache per thread, avoiding a global lock.
Numerical validation includes 810 vendored spring samples, 48 settling durations,
and 2,700 comparisons against iterative physics spanning damping regimes,
frame rates, and fractional steps, with a 1e-9 tolerance.

## Native export versus browser rendering

```sh
npm ci --prefix benchmarks/remotion --ignore-scripts --no-audit --no-fund
cargo build --locked --release -p rust-motion-core --example render_bench
IMAGE_FORMAT=png node benchmarks/remotion/render-bench.mjs
IMAGE_FORMAT=jpeg node benchmarks/remotion/render-bench.mjs
```

The default Chrome path is the macOS application path. Set `CHROME_PATH` to use
another Chrome installation. Set `REPEATS` to change the default three measured
renders. Output media and browser bundles go under `target/remotion-benchmark-*`.
Pinned npm versions and their lockfile are isolated under `benchmarks/remotion`;
production Rust Motion gains no Node dependency.

### Three.js lifecycle smoke test

`apps/studio-tauri/src/three-lifecycle-composition.js` demonstrates the
framework-free `setup/render/dispose` contract. With the Studio Vite server
running, validate composition registration, persistent scene state, frame
rendering, and screenshot capture with:

```sh
node benchmarks/browser/three-lifecycle-smoke.mjs
```

The native 3D emission path can be measured independently of rasterization
and FFmpeg with:

```sh
cargo run --locked --release -p rust-motion-composition --example mesh3d_bench
```

Both scene definitions render 32 opaque moving rectangles at 1280x720, 30 fps,
180 frames, using the same spring parameters. Each uses four render workers,
H.264, CRF 18, and the fast x264 preset. Both use the same system FFmpeg binary;
Remotion's bundled FFmpeg lacks the rawvideo demuxer required by Rust Motion's pipe.
Remotion keeps its own compositor libraries via `binariesDirectory`.

One warmup render per engine is excluded. Chromium is reused, and bundling and
browser startup are reported separately rather than charged to Remotion's timed
renders. Native timings include process startup and encoding. Render order
alternates. The PNG path compares lossless intermediate frames; the JPEG path
uses Remotion's JPEG screenshot default and therefore permits an additional
lossy stage. JPEG can also mark H.264 output as full range (`yuvj420p`).

Output gates compare eight PNG frames byte-for-byte after RGBA decoding, verify
both MP4s contain all 180 frames at the expected dimensions/rate/codec, and
compare every decoded video frame. SSIM comparisons normalize the output color
range to limited-range YUV420P; every frame must score at least 0.995. The first
PNG measurement had SSIM 1.0 on all 180 frames. Checks and raw timings are saved
in `render-*.json`. These are warm-export scene benchmarks, not a comparison of
all product features, browser startup, or compile/bundle time.

## Interpretation

The currently recorded lossless PNG batch (`render-png.json`) was taken on an
Apple M4 Pro and measured a 0.543s Rust Motion median versus 10.125s for Remotion
(18.66x). This file contains one measured sample per engine, so it is a
directional snapshot rather than a production latency budget. Earlier batches
in this repository report different absolute timings; always use a fresh run
on the target machine when making a performance claim.

This scene does not exercise media decoding or audio, and its browser SVG/CSS
effects are only a representative subset of complex filters. The recorded
paired 1080p result is available in `cyberpunk-bench-result.json`; it measured
1.57x Native speedup on the tested M4 Pro batch. Extend the paired scenes and
output gates before generalizing that result to other workloads. A kernel or
scene speedup must not be presented as the same speedup for every export.

To measure the persistent Three.js/Chromium transport under the same 1280x720,
180-frame workload, start the Studio Vite app and run:

```sh
node benchmarks/browser/three-worker-bench.mjs
```

The current PNG scaling snapshots are recorded in
`browser-three-{1w,2w,4w}.json`: 3.000s / 3.000s / 1.500s for 1 / 2 / 4
workers, or approximately 60 / 60 / 120 FPS equivalent, on the local Apple
Silicon host. This is a transport result for the bundled `three_preview`
composition, not a Remotion speedup claim. The near-linear gain appears only
once four isolated Chromium pages are available; two workers are saturated by
the composition or host.

Set `WORKERS=2` or `WORKERS=4` to measure persistent Chromium worker scaling;
each worker owns an isolated page and receives a different frame in each batch.

For transport comparisons, set `RUST_MOTION_BROWSER_IMAGE_FORMAT=jpeg` and
optionally `RUST_MOTION_BROWSER_JPEG_QUALITY=90`. On the reference M4 Pro
Three.js scene, JPEG quality 90 measured about 2x the PNG throughput (roughly
60 FPS versus 30 FPS), but remains opt-in because it is lossy.

To keep encoded frame bytes out of the JSON/base64 channel, set
`RUST_MOTION_BROWSER_TRANSPORT=file`. The worker writes a process-scoped
temporary PNG/JPEG and the Rust backend removes it after decoding. This mode is
useful for local Tauri/CLI jobs; the default remains JSON/base64 for maximum
portability.

For raw WebGL/Canvas RGBA frames, set `RUST_MOTION_BROWSER_TRANSPORT=rgba_file`.
The worker writes tightly packed RGBA8 bytes to a process-scoped temporary file
and returns only its path, avoiding base64 expansion while preserving lossless
pixel transport. The Rust backend removes the file after reading it.

On the local macOS arm64 host, a repeated 1280x720 / 30-frame / 3-repeat
measurement produced the following browser-only baseline (median sample):

| transport | workers | median | equivalent FPS |
| --- | ---: | ---: | ---: |
| PNG/base64 | 1 | 500.19 ms | 59.98 |
| PNG/base64 | 2 | 499.74 ms | 60.03 |
| PNG/base64 | 4 | 296.42 ms | 101.21 |
| raw RGBA file | 1 | 2339.47 ms | 12.82 |
| raw RGBA file | 2 | 1261.47 ms | 23.78 |
| raw RGBA file | 4 | 773.86 ms | 38.77 |

These numbers measure Chromium transport/render behavior only; they are not a
Remotion end-to-end comparison and should be re-run after renderer changes.

For browser compositions that intentionally render transparency, set
`RUST_MOTION_BROWSER_TRANSPARENT=1`. This forwards Remotion's `omitBackground`
behavior to Playwright for PNG frames; JPEG transport remains opaque by
definition.

Both native and browser backends expose bounded frame-resource caches. Set
`RUST_MOTION_IMAGE_CACHE_BYTES` for decoded native images (default 256 MiB) and
`RUST_MOTION_FRAME_CACHE_BYTES` for browser-rendered RGBA frames (default 512
MiB). These limits are useful when a Tauri window, Dioxus preview, and export
job share one process or machine; invalid and non-positive values use the
default.

Set `OUTPUT=benchmarks/browser-result.json` to persist the raw Chromium worker
sample report. The worker marks headless pages so the Studio preview loop cannot
race an explicit frame request. Validate browser media seeking and timeline
visibility with:

```bash
node benchmarks/browser/media-sync-smoke.mjs
```

The measured M4 Pro batch produced a 10.951s median (16.44 fps equivalent),
compared with 11.041s for the paired Remotion export. This is approximately
1.01x, or parity; it is not comparable to the native 5.04x result because the
Browser backend intentionally pays the Chromium/WebGL cost for ecosystem
compatibility.

## The 3-Axis Battle Benchmark Suite

Run the full end-to-end battle benchmark against Remotion measuring memory footprint, 1080p complex VFX/typography, and cloud serverless costs:

```sh
python3 benchmarks/run_battle.py
```

### 1. Real-Time Memory (RSS) Profiling
```sh
# Profile any render command across its entire process tree (including Chromium workers):
python3 benchmarks/profile_memory.py -o mem_result.json -- target/release/examples/cyberpunk_bench target/test.mp4 4
```

### 2. 1080p Cyberpunk VFX & Typography Benchmark
```sh
# Rust Motion native 1080p VFX:
target/release/examples/cyberpunk_bench target/cyberpunk.mp4 4

# Remotion 1080p VFX (Node.js + Chrome required):
node benchmarks/remotion/render-cyberpunk-bench.mjs
```

### 3. Serverless Cost & Docker Footprint Calculator
```sh
# Calculate AWS Lambda batch costs for 10k & 100k videos:
python3 benchmarks/cost_calculator.py
```

See `benchmarks/BATTLE_REPORT.md` for the latest comprehensive battle report.

### OrbStack/Linux container validation

The native Docker image must be built with the Linux multi-stage Dockerfile;
do not copy `target/release/rust-motion` from macOS or Windows into the image.

Build and run a functional Linux smoke test from the repository root:

```sh
docker build --platform linux/arm64 -f benchmarks/docker/Dockerfile.rust-motion -t rust-motion:latest .
sh benchmarks/docker/smoke.sh
```

The smoke test renders 16 native H.264 frames inside the container and checks
the resulting MP4 with `ffprobe`. Set `IMAGE`, `PLATFORM`, or `OUT_DIR` to
test another image, architecture, or output directory.
From the repository root, OrbStack or Docker can build and execute it with:

```sh
docker build -f benchmarks/docker/Dockerfile.rust-motion -t rust-motion:local .
docker run --rm rust-motion:local render --help
```

The builder compiles `rust-motion-cli` against musl inside Linux and the Alpine
runtime keeps FFmpeg and CA certificates available for native exports.


## Native buffer and export pipeline comparison

`compare-render-pipeline.py` alternates immutable before/after release binaries
on the same machine, with one excluded warmup and five measured runs. It covers
720p/1080p/4K CPU frame generation, a 1080p vignette frame, the 720p spring export,
and the 1080p Cyberpunk export. Compile time and validation are excluded.

Before changing the renderer, build and save the three examples:

```sh
cargo build --locked --release -p rust-motion-rasterizer --example raster_bench \
  -p rust-motion-core --example render_bench --example cyberpunk_bench
mkdir -p target/render-speed/before
cp target/release/examples/{raster_bench,render_bench,cyberpunk_bench} target/render-speed/before/
```

After changing the renderer, rebuild and copy the same executables to
`target/render-speed/pipelined`, then run:

```sh
python3 benchmarks/compare-render-pipeline.py \
  --before target/render-speed/before --after target/render-speed/pipelined \
  --output benchmarks/render-pipeline-result.json
```

Every measured raster run checks all 24 RGBA frame hashes against the baseline.
Both exports must preserve the decoded bytes and order of all 180 video frames.
The report records executable SHA-256 hashes, every timing sample, and medians.
`render-buffer-transfer.json` isolates the buffer-transfer change.
`render-pipeline-loaded.json` records an exploratory run affected by an unrelated
Rust compilation; its export times are not an idle-machine baseline.

The CPU backend now transfers its pixel allocation to the output image and
avoids clearing memory already zeroed by `Pixmap::new`. The pipe renderer issues
a replacement render as soon as an ordered frame is written. Its in-flight
output-frame limit remains the configured concurrency, including frames waiting
for order and the frame being written. Per-node scratch surfaces and FFmpeg's
own buffers are additional, as before. Tests exercise out-of-order completion,
render/write overlap, the frame limit, cancellation, write/render errors, and
worker-panic propagation.
