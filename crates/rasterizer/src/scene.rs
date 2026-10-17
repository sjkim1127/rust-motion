//! Scene graph intermediate representation for native rendering.
//!
//! Compositions write into a [`Scene`] instead of DOM nodes.
//! The rasterizer backend reads the scene and produces pixel output.

use serde::{Deserialize, Serialize};

/// RGBA color: `[red, green, blue, alpha]` each in `0..=255`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    /// Parse a CSS hex color: `"#rrggbb"` or `"#rrggbbaa"`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let s = hex.trim_start_matches('#');
        match s.len() {
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Self::rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                let a = u8::from_str_radix(&s[6..8], 16).ok()?;
                Some(Self::rgba(r, g, b, a))
            }
            _ => None,
        }
    }

    /// Parse common CSS color forms used by Rust Motion primitives.
    pub fn from_css(value: &str) -> Option<Self> {
        let value = value.trim();
        if let Some(color) = Self::from_hex(value) {
            return Some(color);
        }
        match value.to_ascii_lowercase().as_str() {
            "black" => return Some(Self::BLACK),
            "white" => return Some(Self::WHITE),
            "red" => return Some(Self::rgb(255, 0, 0)),
            "green" => return Some(Self::rgb(0, 128, 0)),
            "blue" => return Some(Self::rgb(0, 0, 255)),
            "yellow" => return Some(Self::rgb(255, 255, 0)),
            "cyan" | "aqua" => return Some(Self::rgb(0, 255, 255)),
            "magenta" | "fuchsia" => return Some(Self::rgb(255, 0, 255)),
            "gray" | "grey" => return Some(Self::rgb(128, 128, 128)),
            "orange" => return Some(Self::rgb(255, 165, 0)),
            "purple" => return Some(Self::rgb(128, 0, 128)),
            "transparent" => return Some(Self::TRANSPARENT),
            _ => {}
        }

        let (contents, has_alpha) = if let Some(contents) = value
            .strip_prefix("rgba(")
            .and_then(|value| value.strip_suffix(')'))
        {
            (contents, true)
        } else {
            let contents = value
                .strip_prefix("rgb(")
                .and_then(|value| value.strip_suffix(')'))?;
            (contents, false)
        };
        let components = contents.split(',').map(str::trim).collect::<Vec<_>>();
        if components.len() != if has_alpha { 4 } else { 3 } {
            return None;
        }
        let channel = |value: &str| -> Option<u8> {
            if let Some(percent) = value.strip_suffix('%') {
                let percent = percent.parse::<f32>().ok()?;
                Some((percent.clamp(0.0, 100.0) * 2.55).round() as u8)
            } else {
                Some(value.parse::<f32>().ok()?.clamp(0.0, 255.0).round() as u8)
            }
        };
        let alpha = if has_alpha {
            let value = components[3];
            if let Some(percent) = value.strip_suffix('%') {
                (percent.parse::<f32>().ok()?.clamp(0.0, 100.0) * 2.55).round() as u8
            } else {
                (value.parse::<f32>().ok()?.clamp(0.0, 1.0) * 255.0).round() as u8
            }
        } else {
            255
        };
        Some(Self::rgba(
            channel(components[0])?,
            channel(components[1])?,
            channel(components[2])?,
            alpha,
        ))
    }

    /// Apply opacity (0.0 – 1.0) to this colour.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.a = (self.a as f32 * opacity.clamp(0.0, 1.0)) as u8;
        self
    }

    pub fn to_tiny_skia_color(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

/// A gradient colour stop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub position: f32, // 0.0 – 1.0
    pub color: Color,
}

/// How a raster image is fitted into its destination rectangle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageFit {
    /// Preserve aspect ratio and fill the destination, cropping overflow.
    #[default]
    Cover,
    /// Preserve aspect ratio and show the complete image with transparent letterboxing.
    Contain,
    /// Stretch the image to exactly match the destination.
    Fill,
    /// Keep the image at its natural pixel size, centered and clipped.
    None,
    /// Use the natural size unless the image must be reduced to fit.
    ScaleDown,
}

/// Layer blend mode shared by CPU rendering and SVG preview.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    #[default]
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
    PassThrough,
}

impl BlendMode {
    /// Convert to rust-motion-color BlendMode.
    pub fn to_color_blend_mode(self) -> rust_motion_color::BlendMode {
        match self {
            Self::Normal => rust_motion_color::BlendMode::Normal,
            Self::Dissolve => rust_motion_color::BlendMode::Dissolve,
            Self::Darken => rust_motion_color::BlendMode::Darken,
            Self::Multiply => rust_motion_color::BlendMode::Multiply,
            Self::ColorBurn => rust_motion_color::BlendMode::ColorBurn,
            Self::LinearBurn => rust_motion_color::BlendMode::LinearBurn,
            Self::DarkerColor => rust_motion_color::BlendMode::DarkerColor,
            Self::Lighten => rust_motion_color::BlendMode::Lighten,
            Self::Screen => rust_motion_color::BlendMode::Screen,
            Self::ColorDodge => rust_motion_color::BlendMode::ColorDodge,
            Self::LinearDodge => rust_motion_color::BlendMode::LinearDodge,
            Self::LighterColor => rust_motion_color::BlendMode::LighterColor,
            Self::Overlay => rust_motion_color::BlendMode::Overlay,
            Self::SoftLight => rust_motion_color::BlendMode::SoftLight,
            Self::HardLight => rust_motion_color::BlendMode::HardLight,
            Self::VividLight => rust_motion_color::BlendMode::VividLight,
            Self::LinearLight => rust_motion_color::BlendMode::LinearLight,
            Self::PinLight => rust_motion_color::BlendMode::PinLight,
            Self::HardMix => rust_motion_color::BlendMode::HardMix,
            Self::Difference => rust_motion_color::BlendMode::Difference,
            Self::Exclusion => rust_motion_color::BlendMode::Exclusion,
            Self::Subtract => rust_motion_color::BlendMode::Subtract,
            Self::Divide => rust_motion_color::BlendMode::Divide,
            Self::Hue => rust_motion_color::BlendMode::Hue,
            Self::Saturation => rust_motion_color::BlendMode::Saturation,
            Self::Color => rust_motion_color::BlendMode::Color,
            Self::Luminosity => rust_motion_color::BlendMode::Luminosity,
            Self::PassThrough => rust_motion_color::BlendMode::PassThrough,
        }
    }
}

impl From<photocraft_psd::blend::BlendMode> for BlendMode {
    fn from(m: photocraft_psd::blend::BlendMode) -> Self {
        match m {
            photocraft_psd::blend::BlendMode::Normal => Self::Normal,
            photocraft_psd::blend::BlendMode::Dissolve => Self::Dissolve,
            photocraft_psd::blend::BlendMode::Darken => Self::Darken,
            photocraft_psd::blend::BlendMode::Multiply => Self::Multiply,
            photocraft_psd::blend::BlendMode::ColorBurn => Self::ColorBurn,
            photocraft_psd::blend::BlendMode::LinearBurn => Self::LinearBurn,
            photocraft_psd::blend::BlendMode::DarkerColor => Self::DarkerColor,
            photocraft_psd::blend::BlendMode::Lighten => Self::Lighten,
            photocraft_psd::blend::BlendMode::Screen => Self::Screen,
            photocraft_psd::blend::BlendMode::ColorDodge => Self::ColorDodge,
            photocraft_psd::blend::BlendMode::LinearDodge => Self::LinearDodge,
            photocraft_psd::blend::BlendMode::LighterColor => Self::LighterColor,
            photocraft_psd::blend::BlendMode::Overlay => Self::Overlay,
            photocraft_psd::blend::BlendMode::SoftLight => Self::SoftLight,
            photocraft_psd::blend::BlendMode::HardLight => Self::HardLight,
            photocraft_psd::blend::BlendMode::VividLight => Self::VividLight,
            photocraft_psd::blend::BlendMode::LinearLight => Self::LinearLight,
            photocraft_psd::blend::BlendMode::PinLight => Self::PinLight,
            photocraft_psd::blend::BlendMode::HardMix => Self::HardMix,
            photocraft_psd::blend::BlendMode::Difference => Self::Difference,
            photocraft_psd::blend::BlendMode::Exclusion => Self::Exclusion,
            photocraft_psd::blend::BlendMode::Subtract => Self::Subtract,
            photocraft_psd::blend::BlendMode::Divide => Self::Divide,
            photocraft_psd::blend::BlendMode::Hue => Self::Hue,
            photocraft_psd::blend::BlendMode::Saturation => Self::Saturation,
            photocraft_psd::blend::BlendMode::Color => Self::Color,
            photocraft_psd::blend::BlendMode::Luminosity => Self::Luminosity,
            photocraft_psd::blend::BlendMode::PassThrough => Self::PassThrough,
            photocraft_psd::blend::BlendMode::Unknown(_) => Self::Normal,
        }
    }
}

/// Geometric clip applied to a composited layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClipRegion {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        corner_radius: f32,
    },
    Path {
        d: String,
    },
}

/// How rendered mask nodes are converted to coverage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MaskMode {
    #[default]
    Alpha,
    AlphaInverted,
    Luminance,
    LuminanceInverted,
}

/// After Effects Track Matte mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrackMatteMode {
    /// Target alpha is modulated by matte alpha.
    #[default]
    Alpha,
    /// Target alpha is modulated by inverted matte alpha (1 - alpha).
    AlphaInverted,
    /// Target alpha is modulated by matte luminance (0.2126R + 0.7152G + 0.0722B).
    Luma,
    /// Target alpha is modulated by inverted matte luminance (1 - luma).
    LumaInverted,
}

/// Pixel filter applied to an offscreen layer in declaration order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneFilter {
    Blur {
        sigma: f32,
    },
    Brightness {
        amount: f32,
    },
    Grayscale {
        amount: f32,
    },
    Opacity {
        amount: f32,
    },
    ChromaticAberration {
        offset_x: f32,
        offset_y: f32,
        angle_rad: f32,
    },
    Vignette {
        offset: f32,
        darkness: f32,
        roundness: f32,
    },
    Contrast {
        factor: f32,
    },
    Saturation {
        factor: f32,
    },
    HueRotate {
        degrees: f32,
    },
    Invert {
        amount: f32,
    },
    Tint {
        color: [u8; 4],
        amount: f32,
    },
    Duotone {
        primary: [u8; 4],
        secondary: [u8; 4],
    },
    ColorGrading {
        contrast: f32,
        saturation: f32,
        gamma: f32,
        tint: Option<[u8; 4]>,
    },
    ColorKey {
        key_color: [u8; 4],
        similarity: f32,
        smoothness: f32,
        spill_suppression: f32,
    },
    /// Shutter-angle motion blur.
    ///
    /// Equivalent to Remotion's `<CameraMotionBlur shutterAngle={180} samples={10}>`.
    /// Accumulates `samples` renders at sub-frame time offsets determined by `shutter_angle`
    /// and `shutter_phase`, blending them together with weighted temporal filter kernels.
    CameraMotionBlur {
        /// Opening angle of the virtual camera shutter in degrees (0–360).
        /// 180° = half-frame exposure (cinematic standard). 360° = maximum blur.
        shutter_angle: f32,
        /// Number of sub-frame samples to accumulate. Higher = smoother blur.
        samples: u32,
        /// Phase offset as a fraction of the shutter interval (-0.5 centres on current frame).
        shutter_phase: f32,
        /// Optional motion blur direction angle in degrees (default horizontal 0.0°).
        #[serde(default)]
        angle_deg: Option<f32>,
        /// Temporal filter weighting kernel (default Box).
        #[serde(default)]
        kernel: crate::motion_blur::TemporalFilterKernel,
    },
    /// Directional 2D motion blur along a specific angle and distance.
    DirectionalMotionBlur {
        /// Motion angle in degrees (0 = horizontal, 90 = vertical).
        angle_deg: f32,
        /// Blur length in pixels.
        length: f32,
        /// Sub-samples along the blur vector.
        #[serde(default = "default_motion_samples")]
        samples: u32,
        /// Temporal filter weighting kernel.
        #[serde(default)]
        kernel: crate::motion_blur::TemporalFilterKernel,
    },
    /// After Effects Optical Glow with threshold, exponential falloff, and anamorphic stretch.
    OpticalGlow {
        #[serde(default = "default_glow_threshold")]
        threshold: f32,
        #[serde(default = "default_glow_radius")]
        radius: f32,
        #[serde(default = "default_glow_intensity")]
        intensity: f32,
        #[serde(default = "default_glow_anamorphic")]
        anamorphic_ratio: f32,
        #[serde(default)]
        tint: Option<[u8; 4]>,
        #[serde(default)]
        blend_mode: crate::vfx::GlowBlendMode,
    },
    /// After Effects Displacement Map geometric warping.
    DisplacementMap {
        #[serde(default)]
        channel_x: crate::vfx::DisplacementChannel,
        #[serde(default)]
        channel_y: crate::vfx::DisplacementChannel,
        #[serde(default = "default_displacement_amount")]
        max_horizontal: f32,
        #[serde(default = "default_displacement_amount")]
        max_vertical: f32,
        #[serde(default)]
        wrap_mode: crate::vfx::DisplacementWrap,
        #[serde(default)]
        map_source: Option<Vec<u8>>,
        #[serde(default)]
        map_width: u32,
        #[serde(default)]
        map_height: u32,
    },
    /// Fractal Noise procedural texture overlay filter.
    FractalNoise {
        #[serde(default)]
        noise_type: crate::vfx::FractalNoiseType,
        #[serde(default)]
        fractal_type: crate::vfx::FractalType,
        #[serde(default = "default_contrast_1")]
        contrast: f32,
        #[serde(default)]
        brightness: f32,
        #[serde(default = "default_noise_complexity")]
        complexity: u32,
        #[serde(default = "default_noise_scale")]
        scale_x: f32,
        #[serde(default = "default_noise_scale")]
        scale_y: f32,
        #[serde(default)]
        evolution: f32,
        #[serde(default = "default_noise_seed")]
        seed: u64,
        #[serde(default)]
        blend_mode: BlendMode,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },
    /// Photoshop Curves adjustment filter.
    Curves {
        #[serde(default)]
        master: Vec<[f32; 2]>,
        #[serde(default)]
        red: Vec<[f32; 2]>,
        #[serde(default)]
        green: Vec<[f32; 2]>,
        #[serde(default)]
        blue: Vec<[f32; 2]>,
    },
    /// Photoshop Levels adjustment filter.
    Levels {
        #[serde(default)]
        in_black: f32,
        #[serde(default = "default_one")]
        in_white: f32,
        #[serde(default = "default_one")]
        gamma: f32,
        #[serde(default)]
        out_black: f32,
        #[serde(default = "default_one")]
        out_white: f32,
    },
    /// Photoshop Vibrance and Saturation adjustment filter.
    Vibrance {
        #[serde(default)]
        vibrance: f32,
        #[serde(default)]
        saturation: f32,
    },
    /// Photoshop Color Balance adjustment filter.
    ColorBalance {
        #[serde(default)]
        shadows: [f32; 3],
        #[serde(default)]
        midtones: [f32; 3],
        #[serde(default)]
        highlights: [f32; 3],
        #[serde(default)]
        preserve_luminosity: bool,
    },
    /// Direct Adjustment parameter object from `rust_motion_color::Adjustment`.
    Adjustment(Box<rust_motion_color::adjust::Adjustment>),
    /// Photoshop Box Blur filter.
    BoxBlur {
        radius: f32,
    },
    /// Photoshop Radial Blur (Spin or Zoom).
    RadialBlur {
        amount: f32,
        #[serde(default)]
        spin: bool,
        #[serde(default = "default_half")]
        center_x: f32,
        #[serde(default = "default_half")]
        center_y: f32,
    },
    /// Photoshop Surface Blur (edge-preserving blur).
    SurfaceBlur {
        radius: f32,
        threshold: f32,
    },
    /// Photoshop Unsharp Mask (amount %, radius px, threshold levels).
    UnsharpMask {
        amount: f32,
        radius: f32,
        threshold: f32,
    },
    /// Photoshop Smart Sharpen (amount %, radius px, reduce noise %).
    SmartSharpen {
        amount: f32,
        radius: f32,
        #[serde(default)]
        reduce_noise: f32,
    },
    /// Photoshop High Pass filter (radius in pixels).
    HighPass {
        radius: f32,
    },
    /// Distortion: Twirl vortex around center (angle in degrees).
    Twirl {
        angle: f32,
    },
    /// Distortion: Spherize 3D spherical lens distortion (amount -100..100%).
    Spherize {
        amount: f32,
    },
    /// Distortion: Pinch center pull (amount -100..100%).
    Pinch {
        amount: f32,
    },
    /// Distortion: Ripple periodic wave displacement.
    Ripple {
        amount: f32,
        #[serde(default = "default_ripple_size")]
        size: f32,
    },
    /// Direct PhotoCraft FilterParams algorithm execution.
    PhotoFilter(Box<photocraft_algo::FilterParams>),
    /// Sandboxed WebAssembly (Wasm) filter plugin.
    WasmPlugin {
        /// Name of the plugin file (e.g. "crt_scanlines.wasm").
        plugin_name: String,
        /// Optional raw WASM bytecode if embedded directly.
        #[serde(default)]
        wasm_bytes: Option<Vec<u8>>,
        /// Custom parameters passed to the WASM plugin.
        #[serde(default)]
        params: serde_json::Value,
    },
    /// 3D LUT (Look-Up Table) color transformation (.cube or .3dl).
    Lut3D {
        /// Raw LUT file content (.cube or .3dl format).
        cube_content: String,
        /// Blend amount (0.0 = original, 1.0 = fully graded).
        #[serde(default = "default_one")]
        amount: f32,
        /// Optional interpolation method: Trilinear or Tetrahedral.
        #[serde(default)]
        tetrahedral: bool,
    },
    /// Lens distortion and chromatic aberration correction from lens calibration database.
    LensCorrection {
        /// Radial distortion coefficients [k1, k2, k3, k4].
        #[serde(default)]
        distortion: [f32; 4],
        /// Vignetting compensation coefficients [k1, k2, k3].
        #[serde(default)]
        vignette: [f32; 3],
        /// Lateral chromatic aberration [kr, kb] (red and blue scale relative to green).
        #[serde(default)]
        tca: [f32; 2],
        /// Optical center (default 0.5, 0.5).
        #[serde(default = "default_half_pair")]
        center: [f32; 2],
    },
    /// Film Emulation: procedural film grain and halation.
    FilmEmulation {
        /// Film grain amount (0..100%).
        #[serde(default)]
        grain_amount: f32,
        /// Film grain particle size (0..100%).
        #[serde(default = "default_grain_size")]
        grain_size: f32,
        /// Film grain roughness (0..100%).
        #[serde(default = "default_grain_roughness")]
        grain_roughness: f32,
        /// Stable random seed for grain.
        #[serde(default)]
        grain_seed: u32,
        /// Halation glow intensity around bright edges (0..2).
        #[serde(default)]
        halation_intensity: f32,
        /// Halation threshold for highlights (0..1).
        #[serde(default = "default_halation_threshold")]
        halation_threshold: f32,
        /// Halation radius (spread in pixels).
        #[serde(default = "default_halation_radius")]
        halation_radius: f32,
        /// Halation color tint (typically warm orange/red [255, 60, 20, 255]).
        #[serde(default = "default_halation_tint")]
        halation_color: [u8; 4],
    },
    /// AI Video Matting & Rotoscoping (Green-screen-less background removal).
    Matte {
        /// Contrast threshold for subject edge separation (0..1).
        #[serde(default = "default_half")]
        threshold: f32,
        /// Optional temporal smoothing factor (0..1) against previous frame matte.
        #[serde(default)]
        temporal_smooth: f32,
    },
    /// Content-aware hole filling and object removal.
    Inpaint {
        /// Mask bounding box `[x, y, width, height]` in normalized coordinates (0..1) to remove.
        hole_rect: [f32; 4],
        /// Patch radius for PatchMatch multi-scale synthesis.
        #[serde(default = "default_patch_radius")]
        patch_radius: usize,
    },
    /// Multi-frame temporal recursive noise reduction for video footage.
    TemporalDenoise {
        /// Temporal blending strength (0..0.95).
        #[serde(default = "default_half")]
        strength: f32,
        /// Motion detection threshold to preserve sharp moving edges.
        #[serde(default = "default_motion_threshold")]
        motion_threshold: f32,
    },
}

/// Drop shadow generated from the filtered and masked layer alpha.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_sigma: f32,
    pub color: Color,
}

const fn default_opacity() -> f32 {
    1.0
}

/// Audio source mixed into the encoded output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    /// Local audio or video file containing an audio stream.
    pub src: String,
    /// Source offset in seconds.
    pub start_from: f64,
    /// Composition timeline offset in seconds.
    pub timeline_start: f64,
    /// Optional audible duration on the composition timeline.
    pub duration: Option<f64>,
    /// Linear gain in `0.0..=1.0`.
    pub volume: f64,
    /// Playback speed supported by FFmpeg `atempo` (`0.5..=2.0`).
    pub playback_rate: f64,
    /// Repeat the source when it is shorter than the requested duration.
    pub looped: bool,
    /// Optional dynamic volume keyframes `(timeline_seconds, volume_gain)`.
    #[serde(default)]
    pub volume_keyframes: Vec<(f64, f64)>,
}

impl AudioTrack {
    pub fn new(src: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            start_from: 0.0,
            timeline_start: 0.0,
            duration: None,
            volume: 1.0,
            playback_rate: 1.0,
            looped: false,
            volume_keyframes: Vec::new(),
        }
    }

    pub fn with_volume_keyframes(
        mut self,
        keyframes: impl IntoIterator<Item = (f64, f64)>,
    ) -> Self {
        self.volume_keyframes = keyframes.into_iter().collect();
        self
    }
}

/// Visual style options for audio visualizers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum VisualizerStyle {
    /// Rectangular frequency spectrum bars.
    Bars {
        #[serde(default = "default_bar_count")]
        count: usize,
        #[serde(default = "default_bar_gap")]
        gap: f32,
        #[serde(default = "default_bar_radius")]
        radius: f32,
        #[serde(default)]
        mirror: bool,
    },
    /// Smooth continuous wave / oscilloscope line.
    Wave {
        #[serde(default = "default_wave_stroke_width")]
        stroke_width: f32,
        #[serde(default)]
        filled: bool,
    },
    /// Radial spectrum bars radiating from a central circle (e.g. podcast profile avatar).
    Radial {
        #[serde(default = "default_radial_radius")]
        radius: f32,
        #[serde(default = "default_radial_count")]
        bar_count: usize,
        #[serde(default = "default_radial_bar_length")]
        bar_length: f32,
    },
}

const fn default_bar_count() -> usize {
    32
}
const fn default_bar_gap() -> f32 {
    4.0
}
const fn default_bar_radius() -> f32 {
    2.0
}
const fn default_wave_stroke_width() -> f32 {
    3.0
}
const fn default_radial_radius() -> f32 {
    60.0
}
const fn default_radial_count() -> usize {
    48
}
const fn default_radial_bar_length() -> f32 {
    50.0
}

impl Default for VisualizerStyle {
    fn default() -> Self {
        Self::Bars {
            count: default_bar_count(),
            gap: default_bar_gap(),
            radius: default_bar_radius(),
            mirror: false,
        }
    }
}

/// A node in the scene graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneNode {
    /// Solid-coloured rectangle with optional corner radius.
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: Color,
        stroke: Option<Color>,
        stroke_width: f32,
        corner_radius: f32,
    },

    /// Circle.
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        fill: Color,
        stroke: Option<Color>,
        stroke_width: f32,
    },

    /// Arbitrary SVG path string (M, L, C, Q, Z commands).
    Path {
        d: String,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
        opacity: f32,
    },

    /// Single-line text.
    Text {
        x: f32,
        y: f32,
        content: String,
        font_size: f32,
        color: Color,
        font_weight: u16, // 400 = normal, 700 = bold
        /// Ordered local TTF/OTF sources. Missing glyphs fall through to the
        /// next source and finally the renderer's system fallback.
        #[serde(default)]
        font_sources: Vec<String>,
    },

    /// Local raster image asset. `src` may be a filesystem path or `file://` URI.
    Image {
        src: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fit: ImageFit,
        opacity: f32,
    },

    /// A decoded frame from a local video file at `time` seconds.
    Video {
        src: String,
        time: f64,
        /// Repeat the video timeline after the source duration.
        #[serde(default)]
        looped: bool,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fit: ImageFit,
        opacity: f32,
        /// Frame interpolation mode when slowing down or time remapping (Repeat, Blend, PixelMotion).
        #[serde(default)]
        interpolation: crate::time_remap::FrameInterpolationMode,
        /// Optional arbitrary non-linear cubic time remapping curve.
        #[serde(default)]
        time_remap: Option<crate::time_remap::TimeRemapCurve>,
    },

    /// Non-visual audio track collected by the output encoder.
    Audio { track: AudioTrack },

    /// Linear gradient background covering a rectangle.
    LinearGradient {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        angle_deg: f32,
        stops: Vec<GradientStop>,
    },

    /// Radial gradient background.
    RadialGradient {
        cx: f32,
        cy: f32,
        r: f32,
        stops: Vec<GradientStop>,
    },

    /// Group with a 2D transform applied to all children.
    Group {
        transform: Transform2D,
        opacity: f32,
        children: Vec<SceneNode>,
    },

    /// Offscreen compositing boundary with filters, clipping, masking, shadow,
    /// and a destination blend mode.
    Layer {
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default)]
        blend_mode: BlendMode,
        #[serde(default)]
        clip: Option<ClipRegion>,
        #[serde(default)]
        mask: Option<Vec<SceneNode>>,
        #[serde(default)]
        mask_mode: MaskMode,
        #[serde(default)]
        filters: Vec<SceneFilter>,
        #[serde(default)]
        shadow: Option<SceneShadow>,
        children: Vec<SceneNode>,
    },

    /// After Effects Track Matte: modulates target layer opacity with a matte layer.
    TrackMatte {
        target: Box<SceneNode>,
        matte: Box<SceneNode>,
        #[serde(default)]
        mode: TrackMatteMode,
    },

    /// After Effects Null Object: an invisible guide layer that applies its
    /// spatial transform and opacity to all child layers, but draws no pixels.
    NullObject {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        transform: Transform2D,
        #[serde(default = "default_opacity")]
        opacity: f32,
        children: Vec<SceneNode>,
    },

    /// Animated GIF scene node.
    ///
    /// Equivalent to Remotion's `<Gif>` from `@remotion/gif`.
    /// The appropriate GIF frame is selected based on `time` (seconds into the
    /// animation, already scaled by `playback_rate` by the caller).
    Gif {
        /// File path of the GIF asset.
        src: String,
        /// Time in seconds into the GIF playback (caller scales by playback_rate).
        #[serde(default)]
        time: f64,
        x: f32,
        y: f32,
        /// Rendered width in pixels.
        w: f32,
        /// Rendered height in pixels.
        h: f32,
        /// Playback speed multiplier (`1.0` = normal speed, `2.0` = double speed).
        #[serde(default = "default_playback_rate")]
        playback_rate: f32,
        /// Behaviour when the animation reaches its last frame.
        #[serde(default)]
        loop_behavior: crate::gif_cache::LoopBehavior,
        /// How the image fits within the bounding box.
        #[serde(default)]
        fit: ImageFit,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },

    /// Standalone full-color 32-bit RGBA emoji.
    Emoji {
        /// Unicode emoji sequence (e.g. "🔥", "🚀", "💡").
        emoji: String,
        x: f32,
        y: f32,
        /// Bounding size in pixels.
        size: f32,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },

    /// Headless Lottie animation scene node (JSON format).
    ///
    /// Powered by `rasterlottie` and `tiny-skia`. Synced to the composition timeline.
    Lottie {
        /// File path or `file://` URI to the Lottie JSON file.
        src: String,
        /// Time in seconds into the Lottie playback.
        #[serde(default)]
        time: f64,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default = "default_playback_rate")]
        playback_rate: f32,
        #[serde(default)]
        loop_behavior: crate::gif_cache::LoopBehavior,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },

    /// Audio frequency spectrum or waveform visualizer (Bars, Wave, Radial).
    AudioVisualizer {
        /// File path or `file://` URI to audio file (MP3, WAV, AAC, M4A).
        src: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        #[serde(default)]
        style: VisualizerStyle,
        #[serde(default)]
        time: f64,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },

    /// Custom GPU/Shadertoy fragment shader pass (WGSL).
    Shader {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        /// WGSL fragment shader code or Shadertoy-compatible mainImage.
        source: String,
        /// Elapsed animation time in seconds (e.g. `iTime`).
        #[serde(default)]
        time: f32,
        /// 4-component uniform vector (`iParams`).
        #[serde(default)]
        params: [f32; 4],
        #[serde(default = "default_opacity")]
        opacity: f32,
    },

    /// Procedural Fractal Noise generator node.
    FractalNoise {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default)]
        noise_type: crate::vfx::FractalNoiseType,
        #[serde(default)]
        fractal_type: crate::vfx::FractalType,
        #[serde(default = "default_contrast_1")]
        contrast: f32,
        #[serde(default)]
        brightness: f32,
        #[serde(default = "default_noise_complexity")]
        complexity: u32,
        #[serde(default = "default_noise_scale")]
        scale_x: f32,
        #[serde(default = "default_noise_scale")]
        scale_y: f32,
        #[serde(default)]
        evolution: f32,
        #[serde(default = "default_noise_seed")]
        seed: u64,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },

    /// Natural paint brush animated stroke (write-on animation).
    AnimatedStroke {
        /// Brush stroke data containing brush settings and sampled points.
        stroke: photocraft_paint::Stroke,
        /// Fraction of the stroke completed in `0.0..=1.0` (write-on timeline progress).
        #[serde(default = "default_one")]
        progress: f32,
        /// Optional color override (if None, uses stroke.brush.color).
        #[serde(default)]
        color: Option<Color>,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },

    /// Professional Photoshop-grade vector shape with boolean operations, fill, stroke, and winding rules.
    VectorShape {
        path: photocraft_doc::Path,
        fill: Option<Color>,
        #[serde(default)]
        stroke_color: Option<Color>,
        #[serde(default)]
        stroke_width: f32,
        #[serde(default = "default_opacity")]
        opacity: f32,
    },
}

const fn default_one() -> f32 {
    1.0
}

const fn default_playback_rate() -> f32 {
    1.0
}

const fn default_motion_samples() -> u32 {
    16
}

const fn default_glow_threshold() -> f32 {
    0.5
}

const fn default_glow_radius() -> f32 {
    20.0
}

const fn default_glow_intensity() -> f32 {
    1.5
}

const fn default_glow_anamorphic() -> f32 {
    1.0
}

const fn default_displacement_amount() -> f32 {
    20.0
}

const fn default_contrast_1() -> f32 {
    1.0
}

const fn default_noise_complexity() -> u32 {
    4
}

const fn default_noise_scale() -> f32 {
    100.0
}

const fn default_noise_seed() -> u64 {
    42
}

const fn default_half() -> f32 {
    0.5
}

const fn default_ripple_size() -> f32 {
    10.0
}

const fn default_grain_size() -> f32 {
    25.0
}

const fn default_grain_roughness() -> f32 {
    50.0
}

const fn default_halation_threshold() -> f32 {
    0.75
}

const fn default_halation_radius() -> f32 {
    12.0
}

const fn default_halation_tint() -> [u8; 4] {
    [255, 60, 20, 255]
}

const fn default_half_pair() -> [f32; 2] {
    [0.5, 0.5]
}

const fn default_patch_radius() -> usize {
    3
}

const fn default_motion_threshold() -> f32 {
    10.0
}

/// Affine 2D transform (scale + rotation + translation).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform2D {
    pub tx: f32,
    pub ty: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotate_deg: f32,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            tx: 0.0,
            ty: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotate_deg: 0.0,
        }
    }
}

impl Transform2D {
    /// Returns the identity transform (no translation, unit scale, 0 rotation).
    pub fn identity() -> Self {
        Self::default()
    }

    /// Creates a translation transform.
    pub fn translate(tx: f32, ty: f32) -> Self {
        Self {
            tx,
            ty,
            ..Self::default()
        }
    }

    /// Creates a uniform or non-uniform scale transform.
    pub fn scale(scale_x: f32, scale_y: f32) -> Self {
        Self {
            scale_x,
            scale_y,
            ..Self::default()
        }
    }

    /// Creates a uniform scale transform.
    pub fn scale_uniform(factor: f32) -> Self {
        Self::scale(factor, factor)
    }

    /// Creates a rotation transform in degrees.
    pub fn rotate(deg: f32) -> Self {
        Self {
            rotate_deg: deg,
            ..Self::default()
        }
    }

    /// Sets or adds translation along the X and Y axes.
    pub fn with_translate(mut self, tx: f32, ty: f32) -> Self {
        self.tx = tx;
        self.ty = ty;
        self
    }

    /// Sets or adds scale along X and Y axes.
    pub fn with_scale(mut self, scale_x: f32, scale_y: f32) -> Self {
        self.scale_x = scale_x;
        self.scale_y = scale_y;
        self
    }

    /// Sets uniform scale.
    pub fn with_scale_uniform(self, factor: f32) -> Self {
        self.with_scale(factor, factor)
    }

    /// Sets rotation in degrees.
    pub fn with_rotate(mut self, deg: f32) -> Self {
        self.rotate_deg = deg;
        self
    }

    /// Combines transforms in sequential order: translate, scale, then rotate.
    pub fn make_transform(
        translate: Option<(f32, f32)>,
        scale: Option<(f32, f32)>,
        rotate_deg: Option<f32>,
    ) -> Self {
        let mut t = Self::identity();
        if let Some((tx, ty)) = translate {
            t = t.with_translate(tx, ty);
        }
        if let Some((sx, sy)) = scale {
            t = t.with_scale(sx, sy);
        }
        if let Some(deg) = rotate_deg {
            t = t.with_rotate(deg);
        }
        t
    }

    pub fn to_tiny_skia(&self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_translate(self.tx, self.ty)
            .post_scale(self.scale_x, self.scale_y)
            .post_rotate(self.rotate_deg)
    }
}

/// The root container for a single video frame's scene.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub nodes: Vec<SceneNode>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, node: SceneNode) {
        self.nodes.push(node);
    }

    /// Collect audio declared in this scene, including nested groups and layers.
    pub fn audio_tracks(&self) -> Vec<AudioTrack> {
        fn collect(nodes: &[SceneNode], parent_volume: f64, output: &mut Vec<AudioTrack>) {
            for node in nodes {
                match node {
                    SceneNode::Audio { track } => {
                        let mut track = track.clone();
                        track.volume *= parent_volume;
                        if (parent_volume - 1.0).abs() > f64::EPSILON {
                            for (_, vol) in &mut track.volume_keyframes {
                                *vol *= parent_volume;
                            }
                        }
                        output.push(track);
                    }
                    SceneNode::Group {
                        opacity, children, ..
                    } => collect(children, parent_volume * f64::from(*opacity), output),
                    SceneNode::Layer {
                        opacity, children, ..
                    } => collect(children, parent_volume * f64::from(*opacity), output),
                    _ => {}
                }
            }
        }

        let mut tracks = Vec::new();
        collect(&self.nodes, 1.0, &mut tracks);
        tracks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_from_hex() {
        let c = Color::from_hex("#ff6600").unwrap();
        assert_eq!(c.r, 0xff);
        assert_eq!(c.g, 0x66);
        assert_eq!(c.b, 0x00);
        assert_eq!(c.a, 0xff);
    }

    #[test]
    fn css_rgb_and_rgba_colors_are_parsed() {
        assert_eq!(
            Color::from_css("rgb(255, 0, 128)"),
            Some(Color::rgb(255, 0, 128))
        );
        assert_eq!(
            Color::from_css("rgba(100%, 0%, 0%, 0.5)"),
            Some(Color::rgba(255, 0, 0, 128))
        );
    }

    #[test]
    fn common_css_named_colors_are_parsed() {
        assert_eq!(Color::from_css("red"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(Color::from_css("blue"), Some(Color::rgb(0, 0, 255)));
        assert_eq!(Color::from_css("fuchsia"), Some(Color::rgb(255, 0, 255)));
    }

    #[test]
    fn test_scene_push() {
        let mut scene = Scene::new();
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            fill: Color::rgb(255, 0, 0),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });
        assert_eq!(scene.nodes.len(), 1);
    }

    #[test]
    fn image_node_round_trips_through_json() {
        let scene = Scene {
            nodes: vec![SceneNode::Image {
                src: "assets/card.png".into(),
                x: 10.0,
                y: 20.0,
                w: 320.0,
                h: 180.0,
                fit: ImageFit::Contain,
                opacity: 0.75,
            }],
        };

        let json = serde_json::to_string(&scene).unwrap();
        assert!(json.contains("contain"));
        assert_eq!(serde_json::from_str::<Scene>(&json).unwrap(), scene);
    }

    #[test]
    fn video_loop_defaults_to_false_for_existing_scene_json() {
        let json = r#"{
            "Video": {
                "src": "clip.mp4", "time": 0.0,
                "x": 0.0, "y": 0.0, "w": 10.0, "h": 10.0,
                "fit": "cover", "opacity": 1.0
            }
        }"#;
        let node = serde_json::from_str::<SceneNode>(json).unwrap();
        assert!(matches!(node, SceneNode::Video { looped: false, .. }));
    }

    #[test]
    fn text_font_sources_default_for_existing_scene_json() {
        let json = r##"{
            "Text": {
                "x": 0.0, "y": 20.0, "content": "legacy",
                "font_size": 18.0, "color": {"r":255,"g":255,"b":255,"a":255},
                "font_weight": 400
            }
        }"##;
        let node = serde_json::from_str::<SceneNode>(json).unwrap();

        assert!(matches!(node, SceneNode::Text { font_sources, .. } if font_sources.is_empty()));
    }

    #[test]
    fn nested_audio_tracks_inherit_group_opacity() {
        let scene = Scene {
            nodes: vec![SceneNode::Group {
                transform: Transform2D::default(),
                opacity: 0.5,
                children: vec![SceneNode::Audio {
                    track: AudioTrack::new("sound.wav"),
                }],
            }],
        };

        let tracks = scene.audio_tracks();
        assert_eq!(tracks.len(), 1);
        assert!((tracks[0].volume - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn nested_audio_tracks_inherit_layer_opacity() {
        let mut track = AudioTrack::new("voice.wav");
        track.volume = 0.8;
        let scene = Scene {
            nodes: vec![SceneNode::Layer {
                opacity: 0.25,
                blend_mode: BlendMode::Normal,
                clip: None,
                mask: None,
                mask_mode: MaskMode::Alpha,
                filters: Vec::new(),
                shadow: None,
                children: vec![SceneNode::Audio { track }],
            }],
        };

        assert!((scene.audio_tracks()[0].volume - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn layer_deserialization_defaults_compositing_options() {
        let node: SceneNode = serde_json::from_str(r#"{"Layer":{"children":[]}}"#).unwrap();
        assert!(matches!(
            node,
            SceneNode::Layer {
                opacity,
                blend_mode: BlendMode::Normal,
                mask_mode: MaskMode::Alpha,
                ..
            } if (opacity - 1.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn transform2d_builder_methods() {
        let t = Transform2D::identity()
            .with_translate(100.0, 200.0)
            .with_scale(2.0, 3.0)
            .with_rotate(45.0);

        assert_eq!(t.tx, 100.0);
        assert_eq!(t.ty, 200.0);
        assert_eq!(t.scale_x, 2.0);
        assert_eq!(t.scale_y, 3.0);
        assert_eq!(t.rotate_deg, 45.0);

        let t_direct = Transform2D::translate(10.0, 20.0);
        assert_eq!(t_direct.tx, 10.0);
        assert_eq!(t_direct.ty, 20.0);
        assert_eq!(t_direct.scale_x, 1.0);

        let t_scale_uni = Transform2D::scale_uniform(1.5);
        assert_eq!(t_scale_uni.scale_x, 1.5);
        assert_eq!(t_scale_uni.scale_y, 1.5);

        let t_rot = Transform2D::rotate(90.0);
        assert_eq!(t_rot.rotate_deg, 90.0);

        let t_combo =
            Transform2D::make_transform(Some((50.0, -50.0)), Some((0.5, 0.5)), Some(180.0));
        assert_eq!(t_combo.tx, 50.0);
        assert_eq!(t_combo.ty, -50.0);
        assert_eq!(t_combo.scale_x, 0.5);
        assert_eq!(t_combo.rotate_deg, 180.0);
    }
}
