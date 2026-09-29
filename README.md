<p align="center">
  <img src="./assets/logo.svg" alt="Rust Motion" width="100%" />
</p>

<p align="center">
  <b>Browser-free, code-driven video rendering in Rust, with optional Dioxus authoring and preview.</b>
</p>

<p align="center">
  <a href="https://github.com/sjkim1127/rust-motion/actions/workflows/ci.yml"><img src="https://github.com/sjkim1127/rust-motion/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-4ec9b0?style=flat-square" alt="License" /></a>
  <a href="https://dioxuslabs.com/"><img src="https://img.shields.io/badge/Dioxus-0.6-e05c4b?style=flat-square&logo=rust&logoColor=white" alt="Dioxus 0.6" /></a>
  <img src="https://img.shields.io/badge/status-early%20development-f59e0b?style=flat-square" alt="Early development" />
</p>

Rust Motion is an early-stage programmatic video toolkit written in Rust. Its current published Rust and Python packages retain the dioxuscut naming. Its native export path renders a registered `NativeComposition` into a small scene graph, rasterizes frames with `tiny-skia` or `wgpu`, and streams a bounded window of ordered RGBA frames to FFmpeg while rendering subsequent frames. Local video frames are decoded through FFmpeg, and declared audio tracks are mixed into the encoded output. The same native scene can be displayed in the Dioxus Player through `NativeCompositionPreview`.

The repository also contains Dioxus timeline, media, shape, transition, player, and Studio-preview components. The opt-in `dioxuscut-vdom` adapter can translate ordinary Dioxus elements, a documented CSS subset, text, local media elements, and basic SVG shapes into the native scene graph. Existing explicit `Scene` and `SceneEmitter` APIs remain available for precise rendering control.

## What works today

- Native scene graph with rectangles, circles, paths, shaped text, local raster images, decoded video frames, audio tracks, gradients, and transformed groups.
- CPU rendering through `tiny-skia`.
- Experimental GPU rendering through `wgpu` for rectangles, circles, tessellated paths, strokes, transformed groups, gradients with up to 16 stops, cached image/video/Lottie textures, and text atlas draws; unsupported node combinations still use CPU fallback.
- Bounded-memory parallel frame rendering into an FFmpeg stdin pipe, including
  persistent Chromium workers for Browser compositions.
- Cached FFprobe metadata and persistent, bounded FFmpeg rawvideo decoder sessions.
- Registry-based Rust compositions and optional sandboxed Rhai compositions, both with JSON props.
- Named frame events for retiming compositions against voice-over audio in Studio; see the [time event guide](docs/time-events.md).
- Shared native composition contract for CLI export and Dioxus Player/Studio preview.
- Dioxus 0.6 `VirtualDom` mutation renderer with block, Flexbox, and Grid layout through Taffy.
- Composable `SceneEmitter` adapters for media, procedural shapes, kinetic captions, fitted multiline text, fades, slides, sequences, freezes, and composited layers.
- Player media synchronization for seek, pause/play, buffering, volume, rate, looping, timeline offsets, and drift correction.
- H.264, H.265, VP9, AV1, ProRes, and GIF video output plus direct PNG, JPEG, and WebP still rendering.
- FFmpeg audio trim, timeline delay, volume, playback-rate, looping, and multi-track mixing with container-appropriate AAC, Opus, or PCM muxing.
- Inclusive frame-range rendering, per-frame progress events, timeout control, and cancellation through the library API or CLI `Ctrl-C`.
- Animation, shape, path, caption, noise, timeline, player, server, encoder, and CLI test coverage.
- Dioxus web example and desktop Studio preview shell.

## Architecture

```text
Native export
  RenderRequest
      -> CompositionRegistry, VdomComposition, or compiled Rhai AST
      -> Composition::prepare(props, context)
      -> PreparedComposition::render(frame)
      -> Scene
      -> TinySkiaBackend / WgpuBackend with CPU fallback
      -> bounded ordered RGBA window
      -> FFmpeg + collected audio tracks
      -> MP4 / WebM / MOV / GIF

Dioxus native-scene preview
  CompositionHandle
      -> Composition::prepare(props, context)
      -> PreparedComposition::render(frame)
      -> SceneView (SVG)
      -> Player / Studio

General Dioxus preview
  Composition / Sequence / Freeze / media / shapes
      -> Player
      -> Dioxus web or desktop UI
```

Native compositions share one `Scene` contract between preview and export. `VdomComposition` crosses the Dioxus/native boundary for its supported DOM and CSS subset, while direct `Scene` and `SceneEmitter` compositions bypass that conversion when exact scene control is preferable.

## Workspace

| Package | Purpose |
|---|---|
| `dioxuscut-animation` | Interpolation, easing, springs, and color interpolation |
| `dioxuscut-composition` | Shared native composition contract, registry, and built-in composition |
| `dioxuscut-core` | Dioxus composition timeline, sequence, freeze, Lottie, and hooks |
| `dioxuscut-media` | Native media helpers and scene emitters, with optional Dioxus image, video, and audio components |
| `dioxuscut-player` | Interactive player, controls, and native Scene preview adapter |
| `dioxuscut-shapes` | Procedural SVG paths and optional Dioxus components |
| `dioxuscut-paths` | SVG path parsing, metrics, and transforms |
| `dioxuscut-captions` | Subtitle parsing, native scene captions, and optional Dioxus components |
| `dioxuscut-noise` | Deterministic simplex noise helpers |
| `dioxuscut-transitions` | Dioxus fade and slide transitions |
| `dioxuscut-rasterizer` | Scene IR, CPU renderer, experimental GPU renderer, FFmpeg pipe |
| `dioxuscut-vdom` | Dioxus VDOM mutation renderer, CSS cascade, Taffy layout, and Scene conversion |
| `dioxuscut-renderer` | Static server and PNG-sequence encoding utilities |
| `dioxuscut-cli` | Render command and Rhai composition runtime |
| `apps/example` | Dioxus web composition preview |
| `apps/studio` | Dioxus desktop preview shell |

## Prerequisites

- A current stable Rust toolchain.
- FFmpeg available on `PATH` for video and GIF output. Direct still rendering does not require FFmpeg.
- A supported native GPU only when using `--backend gpu`.

### Backend selection

The renderer uses one shared timeline, props, frame scheduling, audio, and
encoding contract across hosts. Choose the backend according to the scene:

| Backend | Best for | Current trade-off |
| --- | --- | --- |
| `native` | Fast, low-memory Rust rendering and AI-generated short scenes | Complex media, text, and browser APIs use the CPU renderer |
| `gpu` | Native shapes, gradients, SVG paths, cached image/video/Lottie textures, and text atlas draws | Unsupported shaders and complex compositing combinations still fall back to CPU |
| `browser` | Tauri/Chromium, Three.js, WebGL, Canvas, and Remotion-compatible web APIs | Requires a browser worker and has process/transport overhead |

Media, shapes, and captions expose Dioxus components through an optional
Cargo feature. Their component API remains enabled by default for compatibility,
while the native CLI and Python binding disable those features so headless native
renders do not depend on Dioxus. Dioxus app dependencies enable the component
feature explicitly. Native GPU texture and text paths are being expanded only
when their output can be checked against CPU and browser references.

Install FFmpeg on common platforms:

```bash
brew install ffmpeg                 # macOS
sudo apt-get install -y ffmpeg      # Debian / Ubuntu
choco install ffmpeg -y             # Windows
```

## Quickstart

The standalone CLI ships with the `HelloWorld` native composition:

```bash
printf '%s\n' '{
  "title": "Hello Rust Motion",
  "subtitle": "Bounded native rendering",
  "background_start": "#0f172a",
  "background_end": "#1e1b4b",
  "accent_color": "#6c63ff"
}' > props.json

cargo run -p dioxuscut-cli -- render \
  --composition HelloWorld \
  --props props.json \
  --output output.mp4 \
  --width 1280 \
  --height 720 \
  --fps 30 \
  --duration 150
```

An unknown composition ID or malformed props file fails before FFmpeg starts.

## Rhai compositions

The optional `rhai` feature adds scriptable composition logic while preserving
JSON as the external data contract. Scripts are compiled once per render job,
receive `ctx` and `props`, and return a restricted native scene builder:

```bash
cargo run -p dioxuscut-cli --features rhai -- render \
  --script examples/hello.rhai \
  --props examples/hello-props.json \
  --output rhai-output.mp4 \
  --width 1280 \
  --height 720 \
  --duration 150
```

Each script defines `fn render(ctx, props)`. The context contains `frame`,
exposes `scene()`, `rect`, `round_rect`, `circle`, `text`, `text_bold`, `text_font`, `text_box`, `image`,
`video`, `audio`, `group`, and `interpolate`. Image and video nodes accept local
paths or `file://` URIs and the fit values `cover`, `contain`, `fill`, `none`, and
`scale-down`:

```rhai
output.image(x, y, width, height, "card.png", "contain", 1.0);
output.text_font(x, y, "Pinned font", 48.0, "#ffffff", "assets/Inter-Regular.ttf");
output.text_box(x, y, width, height, "Wrapped text", 48.0, 20.0, 3, "#ffffff", "assets/Inter-Regular.ttf", "center");
output.video(x, y, width, height, "clip.mp4", source_time, "cover", 1.0);
output.video(x, y, width, height, "clip.mp4", source_time, "cover", 1.0, true); // loop
output.audio("clip.mp4", source_offset, timeline_offset, duration, volume, playback_rate, looped);
```

An audio `duration` of `0.0` means the remainder of the composition. For a
native render, explicit render-level audio should be returned through
`PreparedComposition::audio_tracks()`; otherwise the runtime uses the first
rendered scene as a legacy fallback. See [`examples/hello.rhai`](examples/hello.rhai)
for the basic scene API.

The runtime disables module imports and limits operations, call depth,
expression depth, variables, functions, strings, arrays, and maps. It does not
expose direct filesystem, network, clock, or random APIs. Media nodes can request
local files from the renderer, so hosts accepting untrusted scripts should also
validate or restrict media paths. A new Rhai scope is created for every frame so
parallel rendering does not share mutable script state.

## Registering a native composition

Applications can implement the shared composition contract and pass the same composition to CLI export or `NativeCompositionPreview`:

```rust,ignore
use dioxuscut_cli::{execute_render_command_with_registry, RenderRequest};
use dioxuscut_composition::{
    CompositionError, CompositionRegistry, NativeComposition, NativeCompositionContext,
};
use dioxuscut_rasterizer::{Color, Scene, SceneNode};
use serde_json::Value;

struct TitleCard;

impl NativeComposition for TitleCard {
    fn id(&self) -> &str {
        "TitleCard"
    }

    fn render(
        &self,
        frame: u32,
        _props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: context.width as f32,
            h: context.height as f32,
            fill: Color::rgb(frame as u8, 24, 48),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        Ok(scene)
    }
}

// Register TitleCard, construct a RenderRequest, then call:
// execute_render_command_with_registry(&request, &registry).await?;
```

## Rendering a Dioxus VDOM natively

`VdomComposition` creates a fresh `VirtualDom` for each frame and implements the
same `NativeComposition` contract used by the CLI, Player, and Studio. This
keeps parallel frame rendering isolated:

```rust,ignore
use dioxus::prelude::*;
use dioxus_core::VirtualDom;
use dioxuscut_composition::{CompositionRegistry, NativeCompositionContext};
use dioxuscut_vdom::VdomComposition;

#[derive(Clone, PartialEq, Props)]
struct CardProps {
    frame: u32,
}

fn Card(props: CardProps) -> Element {
    rsx! {
        main { class: "card",
            h1 { "Frame {props.frame}" }
            img { src: "assets/poster.png", class: "poster" }
        }
    }
}

let composition = VdomComposition::new(
    "DioxusCard",
    |frame, _props, _context: NativeCompositionContext| {
        VirtualDom::new_with_props(Card, CardProps { frame })
    },
)
.with_css(r#"
    .card {
        display: flex;
        width: 1280px;
        height: 720px;
        padding: 64px;
        gap: 32px;
        background: #0f172a;
        color: white;
    }
    .poster { width: 480px; height: 270px; object-fit: cover; }
"#)?;

let mut registry = CompositionRegistry::new();
registry.register(composition)?;
```

Supported selectors are `tag`, `.class`, `#id`, `*`, comma-separated groups,
and compounds such as `main.card#hero`. The CSS subset covers block, Flexbox,
and Grid sizing and placement; position and inset; margin, padding, and gap;
alignment; solid backgrounds and borders; font size, weight, line height, color,
opacity, overflow clipping, aspect ratio, and media object fit. Inline `style`
and Dioxus style-namespace attributes override stylesheet rules. Complex
combinators, pseudo-selectors, browser APIs, event behavior, and the full CSS
painting model are intentionally outside this adapter.

## Spring playback controls

The vendored Remotion 4.0.495 spring timing behavior is available as native Rust
through `dioxuscut_animation` and the `dioxuscut_core` re-exports:

```rust
use dioxuscut_animation::{measure_spring, spring_with_options, SpringConfig, SpringOptions};

let config = SpringConfig::default();
assert_eq!(measure_spring(30.0, &config, 0.005).unwrap(), 28.0);
let y = spring_with_options(18.5, 30.0, config, SpringOptions {
    from: 100.0,
    to: 0.0,
    delay: 8.0,
    duration_in_frames: Some(30.0),
    reverse: false,
    ..Default::default()
}).unwrap();
```

`spring_with_options` supports fractional and negative frames, custom ranges,
delay, duration scaling, reversal, and a settling threshold. The existing
`spring(frame, fps, config)` signature remains available. `SpringTiming` now
scales its physics curve to its requested transition duration; the example app
uses duration-controlled title scaling and delayed subtitle motion.

Frame evaluation takes constant time; settling durations use a bounded cache
per render thread (64 entries, with no global lock). Both new functions return
`SpringError` for invalid inputs. Settling simulations and supported adjusted
frame indices are limited to 100,000. A measurement threshold of zero returns
infinity; duration scaling and reversal require a threshold strictly between
zero and one. Overshoot clamping applies to normalized progress before range
mapping, so ascending and descending ranges stay within their endpoints. This
intentionally differs from Remotion 4.0.495's clamping for non-unit ranges.
There is no runtime dependency on Node or `vendor`.

Parity fixtures cover 810 spring values and 48 settling durations generated by
executing the vendored TypeScript. Regenerate them from the repository root with
Node 24 using `node crates/animation/tests/fixtures/generate-spring-reference.mjs`;
run them with `cargo test --locked -p dioxuscut-animation --test spring_parity`.

## CLI reference

```text
dioxuscut render [OPTIONS] (--composition <ID> | --script <PATH>)

  -c, --composition <ID>       Registered composition ID
      --script <PATH>          Rhai composition file; requires feature `rhai`
  -p, --props <PATH>           JSON props file
  -o, --output <PATH>          Output path [default: out.mp4]
      --audio <PATH>           Local audio file to mix; may be repeated
      --width <PX>             Even output width [default: 1920]
      --height <PX>            Even output height [default: 1080]
      --fps <FPS>              Finite positive FPS [default: 30]
      --duration <FRAMES>      Positive frame count [default: 150]
      --backend <BACKEND>      native or gpu [default: native]
      --codec <CODEC>          h264, h265, vp9, av1, prores, gif, png, jpeg, or webp [default: h264]
      --frame-start <FRAME>    First composition frame [default: 0]
      --frame-end <FRAME>      Last composition frame, inclusive
      --timeout-seconds <SEC>  Cancel after a positive timeout
      --crf <VALUE>            H.264/H.265: 0-51; VP9/AV1: 0-63 [default: 18]
      --preset <PRESET>        H.264/H.265 encoder preset [default: fast]
      --hw-accel <MODE>        Hardware encoder: auto, disabled, videotoolbox, nvenc [default: auto]
```

The output extension must match the codec: `.mp4` for H.264/H.265, `.webm` for
VP9/AV1, `.mov` for ProRes, and the matching extension for GIF or still images.
Still codecs render `--frame-start` only; omit `--frame-end` or set it to the
same frame. GIF and still outputs do not accept audio tracks. AV1 selects
`libsvtav1` when available and otherwise uses `libaom-av1`.

## Python SDK (`dioxuscut-py`)

Native high-performance video rendering for Python with zero Chromium/Puppeteer overhead:

The native extension uses PyO3 0.29 with the stable Python 3.10 ABI3 target.

```bash
pip install dioxuscut
```

```python
import dioxuscut

# 1. Render dynamic Rhai motion graphics template (4K / 60 FPS)
dioxuscut.render_script(
    "templates/motion.rhai",
    "output.mp4",
    props={"title": "Hello World", "theme": "cyberpunk"},
    width=3840,
    height=2160,
    fps=60.0,
    duration=180,
    hw_accel="videotoolbox"  # Hardware acceleration on Apple Silicon / NVENC on Linux
)

# 2. Render registered Rust composition
dioxuscut.render(
    "HelloWorld",
    "clip.mp4",
    width=1920,
    height=1080,
    fps=30.0,
    duration=90
)

# 3. Export still frame thumbnail
dioxuscut.render_still("HelloWorld", frame=30, output="thumbnail.png")
```

Build GPU support explicitly:

```bash
cargo build -p dioxuscut-cli --features gpu
```

Build Rhai and GPU support together with `--features rhai,gpu`.

## Performance comparisons

New Remotion ports should preserve output and beat the corresponding Remotion
workload before being described as faster. Reproducible spring and MP4-render
comparisons, raw timings, and output checks live in [`benchmarks`](benchmarks/README.md).
The spring benchmark includes Remotion's warm caches; render comparisons reuse
Chromium, share the same FFmpeg binary, and verify pixels and frame counts.
Results apply to the measured scenes and hardware, not all compositions.

## Testing

The project treats default and optional-feature builds as required quality gates:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo check --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-features
```

The render integration tests produce real H.264, H.265, VP9, AV1, ProRes, and
GIF containers when their FFmpeg encoders are installed, and decode direct PNG,
JPEG, and WebP still outputs.

## Releasing

Releases are driven by a version tag such as `v0.2.0`. The release workflow
validates the tag against the Cargo workspace version, tests the complete
workspace, publishes all public `dioxuscut-*` packages to crates.io in dependency
order, builds Rhai-enabled CLI archives for Linux, macOS, and Windows, and then
creates one GitHub Release with SHA-256 checksums.

The Python extension wheels and source distribution are built and published to
PyPI by a separate workflow on the same version tag.

Repository maintainers must configure the crates.io API token as the GitHub
Actions secret `CRATES_IO_TOKEN` and the PyPI API token as `PYPI_API_TOKEN`.
Tokens must never be committed, placed in a tag, or written into workflow files.

## Current limitations

- `dioxuscut-vdom` translates an explicit DOM/CSS subset, not a browser engine. Complex selectors, intrinsic browser layout, general DOM APIs, events, animations, and advanced paint effects still require direct Scene APIs or further adapter work. Basic CSS linear/radial gradients, translate/scale/rotate transforms, borders, non-inset `box-shadow`, common pixel filters (including `drop-shadow`), `mix-blend-mode`, and the cross-backend `CanvasImage` source fallback are supported; arbitrary canvas/WebGL drawing remains a Browser/Three.js concern.
- Native video and audio sources are local files, while image nodes additionally accept local SVG files and base64 `data:image/png`, `data:image/jpeg`, `data:image/webp`, and `data:image/svg+xml` sources up to 32 MiB. SVG assets are rasterized through the same bounded image cache. Project renders can opt into downloading HTTP(S) assets with `--asset-cache-dir`; the CLI caps each asset at 256 MiB and the project at 1 GiB by default (`--max-total-asset-bytes` changes the total). Direct remote URLs are still not opened by the rasterizer itself.
- Video frames use cached FFprobe stream metadata, up to four persistent FFmpeg decoder sources, fixed-output-FPS sampling for VFR input, and a 128 MiB frame LRU. Backward or large forward seeks restart only the affected decoder.
- Audio declarations may be supplied by `PreparedComposition::audio_tracks()` as
  an explicit render-level timeline, including per-track timing and volume
  keyframes. The CLI scans rendered scenes across the full timeline when that
  method returns `None`, so it includes tracks first emitted after frame zero.
  This legacy fallback renders frames once before encoding; implement
  `audio_tracks()` to avoid the extra pass.
- `SceneLayer` supports rectangular or SVG-path clips, alpha or luminance masks, twelve blend modes, ordered blur/brightness/grayscale/opacity filters, and drop shadows. These effects use CPU offscreen surfaces for export and SVG/CSS equivalents for Player preview.
- GPU acceleration covers rectangles, circles, tessellated path fills and strokes, nested group transforms and opacity, plain normal layers, gradients with up to 16 stops, cached image/video/Lottie textures, and text atlas draws. Unsupported shaders, gradients beyond the stop limit, and some composited mask/blend/filter combinations use CPU fallback; the supported texture/text paths have dedicated CPU/GPU parity tests.
- Text nodes accept ordered local TTF/OTF `font_sources`; native rendering caches those files, shapes glyph runs with Rustybuzz, and falls through per grapheme. `SceneTextBlock` and Rhai `text_box` add Unicode line breaking, fitting, alignment, line limits, and ellipsis. Text without explicit sources still uses platform font discovery and is not pixel-identical across platforms; full mixed-direction paragraph layout remains incomplete.
- Studio project loading and Tauri render-queue controls are available; timeline editing and media management are not yet a full editor.

## Roadmap

1. Expand VDOM/CSS conversion beyond the current simple-selector and native-paint subset.
2. Full bidirectional paragraph layout, variable-font axes, and advanced typography controls.
3. Complete GPU parity for text, media, composited layers, masks, blend modes, and filters.
4. Additional color, distortion, and convolution filter primitives.
5. Studio timeline editing and media management on top of the shared project/job contract.

6. A first-class dynamic audio provider for compositions whose audio topology
   changes over time, without requiring a frame-by-frame scene scan.

## License

Licensed under either [Apache License 2.0](LICENSE) or [MIT License](LICENSE-MIT), at your option.
