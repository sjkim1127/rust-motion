<p align="center">
  <img src="./assets/logo.svg" alt="Rust Motion" width="100%" />
</p>

<p align="center">
  <b>A deterministic, browser-free engine for programmatic video. Describe frames in Rust, Rhai, Python, or a Remotion-style TypeScript API, and render them with a native multi-threaded rasterizer.</b>
</p>

<p align="center">
  <a href="https://github.com/sjkim1127/rust-motion/actions/workflows/ci.yml"><img src="https://github.com/sjkim1127/rust-motion/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-4ec9b0?style=flat-square" alt="License" /></a>
  <img src="https://img.shields.io/badge/version-1.0.0-6c63ff?style=flat-square" alt="Version 1.0.0" />
  <a href="docs/stability.md"><img src="https://img.shields.io/badge/API-SemVer%201.x-22c55e?style=flat-square" alt="SemVer 1.x" /></a>
</p>

<p align="center">
  <img src="./assets/showcase.gif" alt="Rust Motion showcase" width="720" />
</p>

---

## Why Rust Motion

Rust Motion turns a function of a frame number into video. It does not drive a web browser: each frame is built as a typed scene graph and rasterized natively, then streamed to FFmpeg.

- **No browser.** No Chromium, Puppeteer, or Electron. Start-up is a process launch, and a render is a single native process.
- **Deterministic.** The CPU rasterizer (`tiny-skia`) is the reference implementation: the same frame index always produces the same pixels, whether rendered sequentially, in reverse, or at random. See [Backend Capabilities](docs/backends.md).
- **Parallel.** Frames are rasterized concurrently across threads and encoded in order. An optional `wgpu` GPU backend and hardware encoders (VideoToolbox, NVENC) are available.
- **Many front ends, one engine.** Rust, Rhai scripts, Python, a Remotion-style TypeScript API, and WebAssembly all produce the same `Scene` IR.
- **Batteries included.** Animation (interpolation, springs, easing), transitions, noise, shapes and SVG paths, text fitting and layout, captions (SRT/VTT/Whisper/ASS), audio DSP, charts, and color tools are separate crates in this workspace.
- **Agent-friendly CLI.** JSON output modes, stable exit codes, a sandbox for media assets, and `validate` / `inspect` / `doctor` commands. See [CLI & Agent Reference](docs/cli.md).

### How it compares to Remotion

Rust Motion is not a drop-in replacement for Remotion. The two make different trade-offs:

| | Remotion | Rust Motion |
|---|---|---|
| Rendering | React renders DOM; headless Chromium captures frames | Scene graph rasterized natively (CPU, optional GPU) |
| What you can draw | Anything a browser can: full CSS, SVG, Canvas, web fonts, React libraries | What the Scene IR supports: shapes, paths, text, images, video, layers, filters, blend modes, Flexbox/Grid layout |
| Authoring | TSX / React components | Rust, Rhai, Python, or the TypeScript SDK (plain objects, **not** JSX/React) |
| Runtime footprint | Node + Chromium | One native binary + FFmpeg |
| Ecosystem | Large (Studio, Lambda, templates, community) | Young and small |

Choose Remotion when you need arbitrary web content or the React ecosystem. Choose Rust Motion when scenes can be expressed in its scene graph and you care about memory, start-up time, determinism, or running many renders per machine. See [Benchmarks](#benchmarks) for what has and has not been measured.

---

## Status

Rust Motion follows [SemVer 1.x](docs/stability.md).

**Stable:** `Scene` / `SceneNode` types, `SceneEmitter` and the temporal containers (`SceneSequence`, `SceneSeries`, `SceneLoop`, `SceneTransitionSeries`), the project schema, the renderer API, the CLI commands `render`, `render-project`, `validate`, `inspect`, `doctor`, `probe`, `serve`, and the TypeScript and Python bindings.

**Experimental** (may change in minor releases):
- the `wgpu` GPU backend (the CPU rasterizer stays the reference),
- the browser worker backend (`--backend browser`),
- `rust-motion migrate`, a heuristic scaffolder for Remotion `.tsx` files — treat its output as a starting point, not a faithful port.

---

## Installation

FFmpeg and FFprobe on `PATH` are needed to encode video. If FFmpeg is missing or fails, an H.264 request falls back to a built-in pure-Rust encoder (H.264 only). Run `rust-motion doctor` to check your setup.

### CLI

```bash
# Prebuilt binary (installs to ~/.local/bin; set RUST_MOTION_INSTALL_DIR to change)
curl -fsSL https://raw.githubusercontent.com/sjkim1127/rust-motion/main/scripts/install.sh | bash

# From crates.io
cargo install --locked rust-motion-cli

# macOS (Homebrew)
brew tap sjkim1127/tap && brew install rust-motion
```

### Libraries

```bash
npm install @rust-motion/remotion   # TypeScript SDK
pip install rust-motion             # Python 3.10-3.13
```

For Rust, depend on the workspace crates you need (for example `rust-motion-composition` and `rust-motion-rasterizer`).

---

## Quick start

Render the bundled Rhai example to a video, using JSON props:

```bash
rust-motion render \
  --script examples/hello.rhai \
  --props examples/hello-props.json \
  --output hello.mp4 \
  --width 1280 --height 720 --fps 30 --duration 90
```

Or render a composition from the built-in registry:

```bash
rust-motion list-compositions
rust-motion render --composition HelloWorld --output video.mp4 --duration 150
```

Useful flags: `--codec` (`h264`, `h265`, `vp9`, `av1`, `prores`, `gif`, `png`, `jpeg`, `webp`), `--crf`, `--preset`, `--hw-accel` (`auto` by default), `--concurrency`, `--frame-start` / `--frame-end`, `--audio <PATH>` (repeatable), and `--sandbox-root <DIR>` to restrict which directories media assets can be read from. Full reference: [docs/cli.md](docs/cli.md). For a guided walkthrough see the [Quickstart](docs/quickstart.md) and [Hello World tutorial](docs/hello-world.md).

---

## Authoring a video

All of these target the same engine. Pick the one that fits your workflow.

### Rhai scripts — templates without compiling

Sandboxed scripts for fast iteration and templating. This is `examples/hello.rhai`:

```rhai
fn render(ctx, props) {
    let output = scene();
    let width = ctx.width.to_float();
    let height = ctx.height.to_float();

    output.rect(0.0, 0.0, width, height, props.background);

    let content = scene();
    let x = interpolate(
        ctx.frame.to_float(),
        0.0,
        (ctx.duration - 1).to_float(),
        -120.0,
        width * 0.12
    );
    content.circle(width * 0.5, height * 0.5, 96.0, props.accent);
    content.text_bold(x, height * 0.48, props.title, 56.0, "#ffffff");
    content.text(x, height * 0.58, props.subtitle, 24.0, "#9ee7ff");

    output.group(content, 0.0, 0.0, 1.0, ctx.progress * 4.0, 1.0);
    output
}
```

Text boxes with automatic fitting and wrapping are also available:

```rhai
// x, y, width, height, text, font_size, min_font_size, max_lines, color, font_file, align
output.text_box(80.0, 120.0, 960.0, 200.0, props.title, 56.0, 24.0, 2, "#ffffff", "assets/Inter-Bold.ttf", "left");
```

### Rust — type-safe scenes

Implement `SceneEmitter` to describe one frame as a list of scene nodes:

```rust
use rust_motion_animation::interpolate::{interpolate, ExtrapolateType, InterpolateOptions};
use rust_motion_animation::spring::{spring, SpringConfig};
use rust_motion_composition::{CompositionError, SceneEmitter, SceneFrameContext};
use rust_motion_rasterizer::{Color, Scene, SceneNode};
use serde_json::Value;

pub struct IntroCard;

impl SceneEmitter for IntroCard {
    fn emit(
        &self,
        ctx: SceneFrameContext,
        _props: &Value,
        scene: &mut Scene,
    ) -> Result<(), CompositionError> {
        let opacity = interpolate(
            ctx.frame as f64,
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

Compose emitters in time with `SceneSequence`, `SceneSeries`, `SceneLoop`, and `SceneTransitionSeries`, and lay out children with the Taffy-based `LayoutBox` (Flexbox and Grid).

### TypeScript — Remotion-style API

`@rust-motion/remotion` mirrors the Remotion API surface for `interpolate`, `spring`, `Easing`, `Composition`, `Sequence`, `Series`, `Loop`, `Freeze`, `AbsoluteFill`, `useCurrentFrame`, and `useVideoConfig`, and renders through the native engine. Components return plain style objects; **there is no React or JSX runtime**, so existing Remotion components with arbitrary DOM/CSS will not run unchanged.

```typescript
import {
  Composition,
  AbsoluteFill,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from '@rust-motion/remotion';
import { renderMedia } from '@rust-motion/remotion/renderer';

const MyVideo = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const opacity = interpolate(frame, [0, 30], [0, 1], { extrapolateRight: 'clamp' });
  const scale = spring({ frame, fps, config: { damping: 12, mass: 0.5, stiffness: 120 } });

  return { style: { ...AbsoluteFill, opacity, transform: `scale(${scale})` } };
};

Composition({
  id: 'IntroVideo',
  component: MyVideo,
  durationInFrames: 180,
  fps: 30,
  width: 1920,
  height: 1080,
});

await renderMedia({
  composition: 'IntroVideo',
  outputLocation: 'out/video.mp4',
  codec: 'h264',
  hwAccel: 'auto',
});
```

More in [`crates/ts/README.md`](crates/ts/README.md). To scaffold a native port from an existing Remotion component (experimental):

```bash
rust-motion migrate Intro.tsx --target rust --output src/intro.rs   # or: rhai, python
```

See the [Remotion Migration Guide](docs/migration-1.0.md) and the [compatibility matrix](docs/compatibility_matrix.md).

### Python

```python
import rust_motion

# Render a registered composition
rust_motion.render("HelloWorld", "clip.mp4", props={"title": "Hello"},
                   width=1920, height=1080, fps=30.0, duration=90)

# Render a Rhai script
rust_motion.render_script("examples/hello.rhai", "output.mp4",
                          props={"title": "Hello", "subtitle": "From Python",
                                 "background": "#0f172a", "accent": "#6c63ff55"},
                          width=3840, height=2160, fps=60.0, duration=180,
                          hw_accel="auto")

# Still frame
rust_motion.render_still("HelloWorld", frame=30, output="thumbnail.png")
```

The module also exposes helpers such as `interpolate`, `spring`, `parse_srt` / `parse_vtt` / `parse_whisper`, `get_video_metadata`, and `get_audio_spectrum`. See the [Python Guide](docs/python.md).

### WebAssembly — in-browser player

`rust-motion-wasm` compiles the engine's math and a canvas player to `wasm32-unknown-unknown` for timeline scrubbing in the browser:

```html
<canvas id="video-canvas"></canvas>
<input type="range" id="scrubber" min="0" max="149" value="0">

<script type="module">
  import init, { WasmPlayer } from './pkg/rust_motion_wasm.js';
  await init();

  // canvas id, width, height, fps, duration in frames
  const player = new WasmPlayer('video-canvas', 1280, 720, 30.0, 150);
  document.getElementById('scrubber').addEventListener('input', (e) => {
    player.seek(Number(e.target.value));
  });

  function loop(timestamp) {
    if (player.is_playing()) player.tick(timestamp);
    requestAnimationFrame(loop);
  }
  requestAnimationFrame(loop);
  player.play();
</script>
```

See [`crates/wasm/README.md`](crates/wasm/README.md) for the full `WasmPlayer` API.

---

## Architecture

```text
 ┌────────────────────────────────────────────────────────────────────────┐
 │                      Compositions & entry points                       │
 │   Rust │ Rhai │ Python │ TypeScript SDK │ WebAssembly │ Project (JSON)  │
 └───────────────────────────────────┬────────────────────────────────────┘
                                     │ evaluate frame N
                                     ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │                   Deterministic scene graph (Scene IR)                 │
 │      Rect, Circle, Path, Text, Image, Video, Layer, Filter, Blend      │
 └───────────────────────────────────┬────────────────────────────────────┘
                  ┌──────────────────┴──────────────────┐
                  ▼                                     ▼
     ┌─────────────────────────┐           ┌─────────────────────────┐
     │ CPU rasterizer          │           │ GPU rasterizer          │
     │ tiny-skia + Rayon       │           │ wgpu (experimental)     │
     │ (reference output)      │           │                         │
     └────────────┬────────────┘           └────────────┬────────────┘
                  └──────────────────┬──────────────────┘
                                     ▼  ordered frames
 ┌────────────────────────────────────────────────────────────────────────┐
 │ Encoder: FFmpeg pipe (libx264/x265, VP9, AV1, ProRes, GIF; VideoToolbox │
 │ / NVENC when available) — or built-in pure-Rust H.264 fallback          │
 │ Stills: PNG, JPEG, WebP                                                │
 └────────────────────────────────────────────────────────────────────────┘
```

---

## Workspace crates

The workspace has many crates; the ones most users touch are:

| Package | Purpose |
|---|---|
| [`rust-motion-cli`](crates/cli) | `rust-motion` binary: render, serve, validate, inspect, probe, doctor, migrate; Rhai runtime |
| [`rust-motion-composition`](crates/composition) | `SceneEmitter`, sequencing, time events, Taffy layout, 3D scene helpers |
| [`rust-motion-rasterizer`](crates/rasterizer) | Scene IR, CPU (`tiny-skia`) and GPU (`wgpu`) rasterizers, filters, text |
| [`rust-motion-animation`](crates/animation) | Interpolation, spring physics, easing, color transitions |
| [`rust-motion-transitions`](crates/transitions) | Fade, slide, wipes, flip, iris, zoom |
| [`rust-motion-shapes`](crates/shapes) / [`paths`](crates/paths) | Procedural shapes; SVG path parsing, length, dashes, morphing |
| [`rust-motion-noise`](crates/noise) | Remotion-compatible simplex noise, fBm, turbulence |
| [`rust-motion-captions`](crates/captions) | SRT / VTT / Whisper / ASS parsing and animated captions |
| [`rust-motion-media`](crates/media) | Media probing, sample extraction, audio visualization |
| [`rust-motion-audio-dsp`](crates/audio-dsp) | Audio effects and dynamics |
| [`rust-motion-charts`](crates/charts) | Animated bar, line, and pie charts |
| [`rust-motion-renderer`](crates/renderer) | Frame-to-video encoding (FFmpeg and pure-Rust fallback), static server |
| [`rust-motion-h264enc`](crates/h264enc) / [`hevc`](crates/hevc) | Pure-Rust H.264 encoder; HEVC support |
| [`rust-motion-project`](crates/project) | `.rust_motion.json` project schema ([format](docs/project-format.md)) |
| [`rust-motion-ts`](crates/ts) | TypeScript SDK (`@rust-motion/remotion`) and its native bridge |
| [`rust-motion-python`](crates/python) | Python SDK (PyO3) |
| [`rust-motion-wasm`](crates/wasm) | WebAssembly build and in-browser player |

The repository also contains image and color-science crates (`algo`, `color`, `cms`, `lensdb`, `psd`, `dng`, and others) used for the computer-vision and color features.

---

## Benchmarks

Measured results live in [`benchmarks/`](benchmarks/) and are reproducible with the scripts there. Only measurements that were actually taken are quoted here; see [`BATTLE_REPORT.md`](benchmarks/BATTLE_REPORT.md) for methodology and caveats (Apple Silicon, macOS).

| Workload | Remotion | Rust Motion |
|---|---|---|
| 720p spring scene, 4 workers/threads — render time | 10.125 s | 1.347 s (≈18.7× faster, paired run) |
| 720p spring scene — peak RSS | not measured | 143.84 MB |
| 1080p text/VFX scene (180 frames) — render time | not measured | 9.683 s (≈18.6 FPS) |
| 1080p text/VFX scene — peak RSS | not measured | 277.77 MB |

This is a small sample (one paired workload), so treat it as indicative rather than a general speedup claim. Serverless cost figures in the report are model outputs from assumed inputs, not measurements. Spring-kernel micro-benchmarks against the real Remotion implementation are in [`benchmarks/README.md`](benchmarks/README.md).

---

## Development

```bash
# Build and test the workspace (the Python crate needs a Python toolchain)
cargo test --workspace --exclude rust-motion-python

# Lint
cargo clippy --workspace --all-targets

# TypeScript SDK tests
node --test crates/ts/tests/*.test.mjs

# Verify version consistency before a release
./scripts/verify-release.sh
```

---

## Documentation

- [Quickstart](docs/quickstart.md) and [Hello World tutorial](docs/hello-world.md)
- [CLI & Agent Reference](docs/cli.md) and [AI-first rendering](docs/ai-first-rendering.md)
- [Python Guide](docs/python.md)
- [Remotion Migration Guide](docs/migration-1.0.md) and [compatibility matrix](docs/compatibility_matrix.md)
- [Backend Capabilities](docs/backends.md) and [hybrid rendering](docs/hybrid-rendering.md)
- [Project Format](docs/project-format.md) and [time events](docs/time-events.md)
- [API Stability](docs/stability.md), [Security](docs/security.md), and [Release Runbook](docs/release-runbook.md)
- [Changelog](CHANGELOG.md)

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE](LICENSE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
