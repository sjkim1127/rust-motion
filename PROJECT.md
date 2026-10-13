# Project: Rust Motion Remotion Native Porting

## Architecture
Rust Motion is a high-performance programmatic video composition and rendering engine built with Rust and Dioxus. The project replaces Remotion TypeScript/WebGL packages with 100% pure Rust implementations, integrating directly into the Rust Motion core timeline, scene graph, tiny-skia CPU rasterizer, and Dioxus reactive UI components with zero runtime dependency on `vendor/`.

```
                    ┌─────────────────────────┐
                    │      apps / studio      │
                    └────────────┬────────────┘
                                 │
             ┌───────────────────┼───────────────────┐
             ▼                   ▼                   ▼
     ┌───────────────┐   ┌───────────────┐   ┌───────────────┐
     │ crates/noise  │   │crates/transit.│   │crates/raster. │
     │ - Simplex 2D-4D│   │ - ClockWipe   │   │ - Visual FX   │
     │ - Mulberry32  │   │ - LinearWipe  │   │   (Vignette,  │
     │ - fBm / Turb. │   │ - Flip / Zoom │   │    Chromatic, │
     │ - <NoiseBg /> │   │ - Easing / Tim│   │    Grading)   │
     └───────┬───────┘   └───────┬───────┘   │ - fit_text_*  │
             │                   │           │ - rounded_box │
             └───────────────────┼───────────┴───────┬───────┘
                                 │                   │
                                 ▼                   ▼
                         ┌───────────────┐   ┌───────────────┐
                         │crates/composit│   │  crates/core  │
                         │ - SceneTrans. │   │ - Prelude     │
                         │ - Timeline    │   │ - Public APIs │
                         └───────────────┘   └───────────────┘
```

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Simplex Noise (2D, 3D, 4D) | Deterministic Simplex noise algorithms with Mulberry32 seeding | M1 | ORIGINAL_REQUEST §R1 |
| 2 | Perlin Gradient Generators | Gradient generators with seedable PRNG (`noise2d`, `noise3d`, `noise4d`) | M1 | ORIGINAL_REQUEST §R1 |
| 3 | Fractal Brownian Motion (fBm) | Multi-octave fBm procedural noise synthesis (`fbm_2d`, `fbm_3d`) | M1 | ORIGINAL_REQUEST §R1 |
| 4 | Turbulent Flow & Domain Warping | Vector path and coordinate domain warping using procedural turbulence | M1 | ORIGINAL_REQUEST §R1 |
| 5 | Dioxus `<NoiseBackground />` | Native Dioxus component generating procedural SVG/canvas noise patterns | M1 | ORIGINAL_REQUEST §R1 |
| 6 | Chromatic Aberration Filter | Offscreen RGB spatial shift filter on `SceneNode::Layer` in `tiny_skia_backend` | M2 | ORIGINAL_REQUEST §R2 |
| 7 | Vignette Filter | Radial/Euclidean/Chebyshev falloff darkening & alpha filter on `SceneNode::Layer` | M2 | ORIGINAL_REQUEST §R2 |
| 8 | Color Grading Suite | Contrast, Saturation, HueRotate, Invert, Tint, Duotone, ColorKey filters | M2 | ORIGINAL_REQUEST §R2 |
| 9 | Presentation & Wipe Transitions | ClockWipe, LinearWipe, Flip, Zoom, Slide, Fade, Iris in `crates/transitions` | M2 | ORIGINAL_REQUEST §R2 |
| 10 | Customizable Easing & Timing | Easing functions, Bézier curves, spring physics in transitions & timeline | M2 | ORIGINAL_REQUEST §R2 |
| 11 | SceneTransitionSeries Integration | Seamless track overlap & composition scheduling in `crates/composition` | M2 | ORIGINAL_REQUEST §R2 |
| 12 | Multi-line Text Auto-scaling | `fit_text_on_n_lines` optimal font size calculation in `crates/rasterizer` | M3 | ORIGINAL_REQUEST §R3 |
| 13 | Bounding-box Fill Layout | `fill_text_box` greedy line-wrapping text fitting in `crates/rasterizer` | M3 | ORIGINAL_REQUEST §R3 |
| 14 | Parametric Rounded Text Boxes | `create_rounded_text_box` multi-corner badge path generator with padding | M3 | ORIGINAL_REQUEST §R3 |
| 15 | Unified Public APIs & Re-exports | Clean public exports in `crates/core`, `crates/rasterizer`, `crates/noise`, etc. | M3 | ORIGINAL_REQUEST §R3 |
| 16 | E2E Requirements Verification | 4-tier opaque-box test suite derivation and execution across all features | M4 | ORIGINAL_REQUEST §Acceptance Criteria |
| 17 | Adversarial Hardening & Final Gate | White-box stress testing, coverage gap elimination, zero warnings, 100% tests | M5 | ORIGINAL_REQUEST §Acceptance Criteria |
| 18 | `makeTransform` & `interpolateStyles` | CSS transform chain builder & style interpolation (`crates/animation`) | M6 | Remotion animation-utils parity |
| 19 | `CameraMotionBlur` Filter | Shutter-angle sub-frame accumulation blur filter in `crates/rasterizer` | M7 | Remotion motion-blur parity |
| 20 | Audio Waveform Visualization | Audio waveform decoding, DFT frequency spectrum, and SVG path generator in `crates/media` | M8 | Remotion media-utils parity |
| 21 | GIF Animation Support | Animated GIF decode cache & frame synchronization (`crates/rasterizer`, `crates/media`) | M9 | Remotion gif parity |
| 22 | Dynamic Font Registry | Dynamic TTF/OTF font registration from bytes/file path (`crates/rasterizer`) | M10 | Remotion fonts parity |
| 23 | Render Cancellation Signal | `make_cancel_signal` and `CancelSignal` token support in `crates/rasterizer` | M11 | Remotion renderer parity |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Native Noise & Procedural Shaders | `crates/noise` (Simplex 2D/3D/4D, Mulberry32, fBm, turbulence, `<NoiseBackground />`) | none | DONE |
| M2 | Visual Effects & Transitions Engine | `crates/rasterizer` (filters), `crates/transitions` (wipes/flips/zoom), `crates/composition` | none | DONE |
| M3 | Typography, Text Fitting & Rounded Boxes | `crates/rasterizer/src/font.rs`, `crates/shapes`, `crates/core` re-exports, formatting | none | DONE |
| M4 | E2E Testing Suite (Tiers 1-4) | Independent E2E test harness & test suite creating `TEST_READY.md` | none | DONE |
| M5 | Final Integration & Adversarial Hardening | Pass 100% of E2E suite, Tier 5 white-box challenger hardening, full workspace check | M1, M2, M3, M4 | DONE |
| M6 | Transform Builder & Style Interpolation | `crates/animation` (`make_transform`, `TransformOp`, `interpolate_styles`) | none | DONE |
| M7 | Camera Motion Blur Filter | `crates/rasterizer` (`SceneFilter::CameraMotionBlur`, `tiny_skia_backend`) | none | DONE |
| M8 | Audio Waveform Visualization | `crates/media` (`AudioData`, `visualize_audio`, `create_smooth_svg_path`) | none | DONE |
| M9 | GIF Media Component & Frame Cache | `crates/rasterizer/src/gif_cache.rs`, `crates/media/src/gif.rs`, `SceneNode::Gif` | none | DONE |
| M10 | Dynamic Font Registration | `crates/rasterizer/src/font.rs` (`register_font_bytes`, `register_font_from_path`) | none | DONE |
| M11 | Render Cancellation Signal | `crates/rasterizer/src/render.rs` (`make_cancel_signal`, `CancelSignal`) | none | DONE |


## Interface Contracts

### `crates/noise` Interface
```rust
pub fn noise2d(seed: impl Into<NoiseSeed>, x: f64, y: f64) -> f64;
pub fn noise3d(seed: impl Into<NoiseSeed>, x: f64, y: f64, z: f64) -> f64;
pub fn noise4d(seed: impl Into<NoiseSeed>, x: f64, y: f64, z: f64, w: f64) -> f64;

pub struct FbmOptions {
    pub octaves: usize,
    pub lacunarity: f64,
    pub persistence: f64,
}
pub fn fbm_2d(seed: impl Into<NoiseSeed>, x: f64, y: f64, options: &FbmOptions) -> f64;
pub fn fbm_3d(seed: impl Into<NoiseSeed>, x: f64, y: f64, z: f64, options: &FbmOptions) -> f64;
pub fn turbulence_warp_2d(seed: impl Into<NoiseSeed>, x: f64, y: f64, strength: f64, freq: f64) -> (f64, f64);

#[component]
pub fn NoiseBackground(props: NoiseBackgroundProps) -> Element;
```

### `crates/rasterizer` ↔ `crates/composition` Filter Interface
```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SceneFilter {
    Blur { radius: f32 },
    Brightness { factor: f32 },
    Grayscale { amount: f32 },
    Opacity { factor: f32 },
    ChromaticAberration { offset_x: f32, offset_y: f32, angle_rad: f32 },
    Vignette { offset: f32, darkness: f32, roundness: f32 },
    Contrast { factor: f32 },
    Saturation { factor: f32 },
    HueRotate { degrees: f32 },
    Invert { amount: f32 },
    Tint { color: [u8; 4], amount: f32 },
    Duotone { primary: [u8; 4], secondary: [u8; 4] },
    ColorGrading { contrast: f32, saturation: f32, gamma: f32, tint: Option<[u8; 4]> },
    ColorKey { key_color: [u8; 4], similarity: f32, smoothness: f32, spill_suppression: f32 },
}
```

### `crates/transitions` Interface
```rust
pub trait TransitionPresentation: Send + Sync {
    fn name(&self) -> &'static str;
    fn render_transition(&self, ctx: &TransitionContext) -> SceneNode;
}

pub struct ClockWipe { pub counter_clockwise: bool, pub start_angle_deg: f32 }
pub struct LinearWipe { pub direction: LinearWipeDirection, pub angle_rad: f32 }
pub struct FlipTransition { pub direction: FlipDirection, pub perspective: f32 }
pub struct ZoomTransition { pub mode: ZoomMode, pub max_scale: f32 }
```

### `crates/rasterizer` Typography & Layout Interface
```rust
pub struct FitTextOnNLinesOptions {
    pub max_lines: usize,
    pub max_box_width: f32,
    pub max_box_height: Option<f32>,
    pub min_font_size: f32,
    pub max_font_size: f32,
}

pub struct TextFitResult {
    pub font_size: f32,
    pub lines: Vec<String>,
    pub total_height: f32,
    pub max_line_width: f32,
}

pub fn fit_text_on_n_lines(
    text: &str,
    font: &Font,
    options: &FitTextOnNLinesOptions,
) -> Result<TextFitResult, LayoutError>;

pub fn fill_text_box(
    text: &str,
    font: &Font,
    font_size: f32,
    max_box_width: f32,
) -> Vec<String>;

pub struct RoundedTextBoxOptions {
    pub padding_x: f32,
    pub padding_y: f32,
    pub border_radius: f32,
    pub align: TextAlign,
}

pub fn create_rounded_text_box(
    lines: &[String],
    font: &Font,
    font_size: f32,
    options: &RoundedTextBoxOptions,
) -> String; // SVG Path 'd'
```

## Code Layout
- `crates/noise/src/`:
  - `lib.rs`: exports and module index
  - `simplex.rs`: 2D, 3D, 4D Simplex noise and Mulberry32 PRNG
  - `fbm.rs`: Fractal Brownian Motion and turbulence domain warping
  - `noise_bg.rs`: Dioxus `<NoiseBackground />` SVG/Canvas component
- `crates/rasterizer/src/`:
  - `scene.rs`: `SceneFilter` enum definition and `SceneNode::Layer` filter list
  - `tiny_skia_backend.rs`: `apply_filter` pixel processing algorithms
  - `font.rs`: typography, `fit_text_on_n_lines`, `fill_text_box`, `create_rounded_text_box`
- `crates/transitions/src/`:
  - `lib.rs`: transition presentations index
  - `clock_wipe.rs`: pie arc clipping transition
  - `linear_wipe.rs`: directional polygon clipping transition
  - `flip.rs`: 3D perspective flip transition
  - `zoom.rs`: zoom and scale blur transition
  - `easing.rs`: easing curves and spring physics
- `crates/core/src/`:
  - `lib.rs`: re-exports of core noise, transitions, layout, and scene traits
- `tests/`:
  - Comprehensive opaque-box E2E tests validating R1, R2, R3 across all crates
