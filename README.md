<p align="center">
  <img src="./assets/logo.svg" alt="Rust Motion" width="100%" />
</p>

<p align="center">
  <b>Ultra-fast, lean, and deterministic programmatic video rendering engine for Rust, TypeScript (Remotion ecosystem), Python, and AI coding agents. 100% headless with zero browser or webview dependencies.</b>
</p>

<p align="center">
  <a href="https://github.com/sjkim1127/rust-motion/actions/workflows/ci.yml"><img src="https://github.com/sjkim1127/rust-motion/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-4ec9b0?style=flat-square" alt="License" /></a>
  <a href="https://www.npmjs.com/package/@rust-motion/remotion"><img src="https://img.shields.io/badge/npm-%40rust--motion%2Fremotion-cb3837?style=flat-square&logo=npm" alt="npm package" /></a>
  <a href="https://pypi.org/project/rust-motion/"><img src="https://img.shields.io/badge/pypi-rust--motion-3775a9?style=flat-square&logo=python" alt="PyPI package" /></a>
  <img src="https://img.shields.io/badge/status-production--ready-22c55e?style=flat-square" alt="Status: Production Ready" />
</p>

---

## ⚡ Highlights

- **Pure Headless Engine**: Zero Chromium, zero Puppeteer, zero WebKit, zero Electron. Boots in milliseconds with ~50MB RSS instead of 2GB+ memory overhead.
- **Remotion 1:1 Parity**: Full TypeScript SDK (`@rust-motion/remotion`) providing drop-in compatibility with `interpolate`, `interpolateColors`, `spring`, `useCurrentFrame`, `useVideoConfig`, `Composition`, `Sequence`, `Series`, and renderer APIs.
- **High-Throughput Parallel Pipeline**: Deterministic `Scene` IR rasterized concurrently across threads using `tiny-skia` (CPU) and `wgpu` (GPU), streaming ordered frames directly to FFmpeg with hardware acceleration (VideoToolbox, NVENC).
- **First-Class Python SDK**: Native PyO3 bindings for data science, AI workflows, and automation pipelines (`pip install rust-motion`).
- **Computer Vision & Color Science Core**: Native Multi-Band Laplacian Pyramid blending, HDR radiance merge and motion deghosting, Brown-Conrady lens distortion calibration, ACES filmic tone mapping, and 3D LUT grading.
- **Kinetic Subtitles & Audio DSP**: Native SRT/VTT/Whisper/ASS caption pagination with word-level spring bounces, ducking curves, and audio frequency visualizers.
- **Agent-First CLI**: High-speed multi-threaded CLI with sandboxed Rhai scripting and an automated Remotion-to-Rust transpiler (`rust-motion migrate`).

---

### 📚 Documentation

- [Quickstart Guide](docs/quickstart.md) — Get up and running in under 5 minutes.
- [Hello World Tutorial](docs/hello-world.md) — Step-by-step composition tutorial.
- [Remotion Migration Guide](docs/migration-1.0.md) — Migrate from Remotion TSX in seconds.
- [CLI & Agent Reference](docs/cli.md) — Complete CLI flags, exit codes, and JSON automation.
- [Python Guide](docs/python.md) — Python SDK, NumPy buffer exports, and usage.
- [Backend Capabilities](docs/backends.md) — Determinism guarantees, CPU reference, and GPU acceleration.
- [Project Format](docs/project-format.md) — `.rust_motion.json` schema specification.
- [API Stability](docs/stability.md) — SemVer 1.x policy and OS support matrix.
- [Security & Advisories](docs/security.md) — Sandboxing and memory safety model.
- [Release Runbook](docs/release-runbook.md) — Release engineering procedures.

---

## 🚀 Installation

### 🤖 For AI Coding Agents & Automated CI (Recommended)
Installs the pre-compiled standalone binary in 1–2 seconds with zero toolchain dependencies:
```bash
curl -fsSL https://raw.githubusercontent.com/sjkim1127/rust-motion/main/scripts/install.sh | bash
```
> Installs to `~/.local/bin/rust-motion` by default. Set `RUST_MOTION_INSTALL_DIR` to customize destination.

### 📦 TypeScript / Remotion Ecosystem (`npm`)
```bash
npm install @rust-motion/remotion
# or
pnpm add @rust-motion/remotion
```

### 🐍 Python Package (`PyPI`)
```bash
pip install rust-motion
# or using uv:
uv tool install rust-motion
```

### 🦀 Cargo (Rust Developers)
```bash
# Fast binary install via cargo-binstall:
cargo binstall rust-motion-cli

# Or compile from source:
cargo install rust-motion-cli --features rhai
```

### 🍺 Homebrew (macOS)
```bash
brew tap sjkim1127/tap
brew install rust-motion
```

---

## 🛠️ Usage Across 5 Ecosystems

### 1. TypeScript & Remotion (`@rust-motion/remotion`)

Write clean, declarative Remotion code with 1:1 API parity, executed by Rust Motion's blazing-fast native engine:

```typescript
import {
  Composition,
  Sequence,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
  renderMedia,
} from '@rust-motion/remotion';

export const MyVideo = () => {
  const frame = useCurrentFrame();
  const { fps, width, height } = useVideoConfig();

  const opacity = interpolate(frame, [0, 30], [0, 1], { extrapolateRight: 'clamp' });
  const scale = spring({ frame, fps, config: { damping: 12, stiffness: 100 } });

  return (
    <Sequence from={0} durationInFrames={90}>
      <div style={{ opacity, transform: `scale(${scale})`, color: '#ffffff' }}>
        Rendered natively with Rust Motion!
      </div>
    </Sequence>
  );
};

// Render directly via high-speed CLI backend:
await renderMedia({
  composition: 'MyVideo',
  outputLocation: 'out.mp4',
  codec: 'h264',
  hardwareAcceleration: 'auto',
});
```

---

### 2. Pure Rust Native Compositions (`rust-motion-composition`)

Create type-safe, zero-cost video scenes implementing `SceneEmitter`:

```rust
use rust_motion_animation::interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
use rust_motion_animation::spring::{spring, SpringConfig};
use rust_motion_composition::{
    CompositionError, NativeComposition, NativeCompositionContext, SceneEmitter,
    SceneEmitterComposition, SceneFrameContext, SceneSequence,
};
use rust_motion_rasterizer::{Color, Scene, SceneNode, Transform2D};
use serde_json::Value;

pub struct IntroCard;

impl SceneEmitter for IntroCard {
    fn emit(
        &self,
        ctx: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let frame = ctx.frame as f64;
        let opacity = interpolate(
            frame,
            &[0.0, 30.0],
            &[0.0, 1.0],
            InterpolateOptions {
                extrapolate_right: ExtrapolateType::Clamp,
                ..Default::default()
            },
        ) as f32;

        let scale = spring(ctx.frame, ctx.composition.fps, SpringConfig::default()) as f32;

        scene.push(SceneNode::Rect {
            x: 100.0,
            y: 100.0,
            w: 400.0 * scale,
            h: 200.0 * scale,
            fill: Color::rgba(255, 230, 0, (opacity * 255.0) as u8),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 16.0,
        });

        Ok(())
    }
}
```

---

### 3. Python SDK (`rust-motion`)

Automate programmatic video rendering directly in Python data pipelines and AI agents:

```python
import rust_motion

# 1. Render dynamic Rhai motion graphics template (4K / 60 FPS)
rust_motion.render_script(
    "templates/motion.rhai",
    "output.mp4",
    props={"title": "Hello World", "theme": "cyberpunk"},
    width=3840,
    height=2160,
    fps=60.0,
    duration=180,
    hw_accel="videotoolbox"  # Hardware encoding on Apple Silicon or NVENC on Linux
)

# 2. Render registered native composition
rust_motion.render(
    "HelloWorld",
    "clip.mp4",
    width=1920,
    height=1080,
    fps=30.0,
    duration=90
)

# 3. Export still frame thumbnail
rust_motion.render_still("HelloWorld", frame=30, output="thumbnail.png")
```

---

### 4. Dynamic Rhai Scripting (`examples/hello.rhai`)

Lightweight, sandboxed scripts for instant template generation without compiling Rust:

```rhai
fn render(ctx, props) {
    let frame = ctx.frame.to_float();
    let output = scene();

    // Background gradient
    output.rect(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), "#0b0d19");

    // Dynamic typography with automatic layout and wrapping
    let title = props.get("title");
    output.text_box(80.0, 120.0, ctx.width.to_float() - 160.0, 200.0, title, 56.0, 24.0, 2, "#ffffff", "assets/Inter-Bold.ttf", "left");

    // Media overlay with cover fitting
    output.image(80.0, 360.0, 640.0, 360.0, "assets/diagram.png", "cover", 1.0);

    output
}
```

---

### 5. WebAssembly & Real-Time In-Browser Player (`rust-motion-wasm`)

Compile the entire video engine, Remotion physics, and TouchDesigner-inspired procedural TOPs directly into WebAssembly (`wasm32-unknown-unknown`) for zero-latency in-browser timeline scrubbing:

```html
<canvas id="video-canvas" width="1280" height="720"></canvas>
<input type="range" id="scrubber" min="0" max="149" value="0">

<script type="module">
  import init, { WasmPlayer, GenerativeCanvas } from './pkg/rust_motion_wasm.js';

  await init();

  // 1. Initialize Player: 1280x720, 30 FPS, 150 frames
  const player = new WasmPlayer('video-canvas', 1280, 720, 30.0, 150);

  // 2. Instant <3ms timeline scrubbing with direct memory blit
  document.getElementById('scrubber').addEventListener('input', (e) => {
    player.seek(Number(e.target.value));
  });

  // 3. Continuous 60 FPS playback
  function loop(timestamp) {
    if (player.is_playing()) player.tick(timestamp);
    requestAnimationFrame(loop);
  }
  requestAnimationFrame(loop);
</script>
```

---

## 🏗️ Architecture

```text
 ┌────────────────────────────────────────────────────────────────────────┐
 │                      Compositions & Entrypoints                        │
 │  TypeScript (Remotion) │ Pure Rust │ Python │ Rhai │ WebAssembly (WASM)│
 └───────────────────────────────────┬────────────────────────────────────┘
                                     │ Evaluates Frame (t)
                                     ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │                  Deterministic Scene Graph (Scene IR)                  │
 │   Rect, Circle, Lyon Path, Text, Image, Video, Layer, Filter, Blend    │
 └───────────────────────────────────┬────────────────────────────────────┘
                                     │
                  ┌──────────────────┴──────────────────┐
                  ▼                                     ▼
     ┌─────────────────────────┐           ┌─────────────────────────┐
     │  CPU Engine (tiny-skia) │           │     GPU Engine (wgpu)   │
     │  Multi-threaded Rayon   │           │  Hardware Offscreen FBO │
     └────────────┬────────────┘           └────────────┬────────────┘
                  │                                     │
                  └──────────────────┬──────────────────┘
                                     ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │                       Parallel Frame Buffer Ring                       │
 │        Lockless bounded queue with backpressure (128MB LRU)            │
 └───────────────────────────────────┬────────────────────────────────────┘
                                     ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │                     Hardware-Accelerated Encoder                       │
 │  FFmpeg Stdin Pipe: VideoToolbox (macOS) / NVENC (Linux) / VAAPI / CPU │
 │  Muxes: H.264, HEVC (H.265), VP9, AV1, ProRes, GIF, PNG, JPEG, WebP   │
 └────────────────────────────────────────────────────────────────────────┘
```

---

## 📦 Workspace Crates

| Package | Purpose |
|---|---|
| [`rust-motion-ts`](crates/ts) | TypeScript/Remotion SDK (`@rust-motion/remotion`) & FFI bridge |
| [`rust-motion-python`](crates/python) | Python SDK & PyO3 native extensions (`pip install rust-motion`) |
| [`rust-motion-wasm`](crates/wasm) | WebAssembly engine & real-time in-browser timeline scrubber player |
| [`rust-motion-cli`](crates/cli) | CLI binary (`rust-motion`), Rhai runner, and Remotion transpiler |
| [`rust-motion-composition`](crates/composition) | Core native scene contracts, timeline sequencing, and emitters |
| [`rust-motion-rasterizer`](crates/rasterizer) | Scene IR, CPU rasterizer (`tiny-skia`), GPU rasterizer (`wgpu`) |
| [`rust-motion-animation`](crates/animation) | Interpolation, spring physics, bezier easing, and color transitions |
| [`rust-motion-media`](crates/media) | Native video/audio sample extractors, metadata probe, audio visualizer |
| [`rust-motion-shapes`](crates/shapes) | Procedural geometric SVG paths (Circle, Rect, Star, Pie, Arrow, etc.) |
| [`rust-motion-paths`](crates/paths) | SVG path parser, length calculation, dash arrays, and morphing |
| [`rust-motion-captions`](crates/captions) | SRT/VTT/Whisper/ASS subtitle parsers and kinetic animated captions |
| [`rust-motion-transitions`](crates/transitions) | Scene transitions (Fade, Slide, ClockWipe, Dissolve, Flip, Iris, Zoom) |
| [`rust-motion-noise`](crates/noise) | Deterministic simplex noise generators |
| [`rust-motion-renderer`](crates/renderer) | Headless render pipeline, frame callbacks, and cancellation signals |
| [`rust-motion-charts`](crates/charts) | Native animated bar, line, and pie charts |
| [`rust-motion-h264enc`](crates/h264enc) / [`hevc`](crates/hevc) | Native bitstream parsers and encoders |
| [`rust-motion-algo`](crates/algo) | Computer vision algorithms: Laplacian pyramid stitching, HDR, deghosting |
| [`rust-motion-lensdb`](crates/lensdb) | Camera & lens calibration database with Brown-Conrady distortion models |

---

## 💻 CLI Commands & Remotion Migration

### Render Composition
```bash
rust-motion render \
  --composition HelloWorld \
  --props props.json \
  --output video.mp4 \
  --width 1920 \
  --height 1080 \
  --fps 30 \
  --duration 150 \
  --codec h264 \
  --hw-accel auto
```

### Transpile Remotion TSX to Native Rust
Migrate existing Remotion React components directly into high-performance pure Rust `SceneEmitter` code:
```bash
rust-motion migrate --input Intro.tsx --target rust --output src/intro.rs
```

### Check System & Hardware Encoders
```bash
rust-motion doctor
```

---

## 📊 Benchmarks: Remotion vs. Rust Motion

Tested on Apple M-series (1080p @ 60 FPS, 300 frames):

| Metric | Remotion (Node/Chromium) | Rust Motion (Pure Headless) | Improvement |
|---|---|---|---|
| **Startup Latency** | 2,450 ms (Chromium launch) | **14 ms** | **175× faster** |
| **Peak Memory (RSS)** | 1,850 MB | **68 MB** | **96% less memory** |
| **Render Speed (CPU)** | 18.2 FPS | **142.5 FPS** | **7.8× faster** |
| **Render Speed (GPU)** | N/A | **310.8 FPS** | **17× faster** |
| **Binary / Bundle Size** | ~450 MB (node_modules + browser) | **18 MB** (single binary) | **25× smaller** |

---

## 🧪 Testing & Verification

```bash
# Run TypeScript SDK tests
node --test crates/ts/tests/*.test.mjs

# Run all workspace unit & integration tests
cargo test --workspace --exclude rust-motion-python

# Verify release consistency
./scripts/verify-release.sh
```

---

## 📄 License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE](LICENSE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
