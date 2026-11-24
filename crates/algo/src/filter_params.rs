//! The [`FilterParams`] enum (every filter and its parameters), its halo rules and labels.

use super::*;
use serde::{Deserialize, Serialize};

/// A filter and its parameters, in Photoshop's dialog units (pixels,
/// percent, degrees, levels 0–255).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "filter",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FilterParams {
    /// Radius = Gaussian standard deviation in pixels (0.1–1000).
    GaussianBlur {
        radius: f32,
    },
    /// Box of `2·radius + 1` pixels.
    BoxBlur {
        radius: f32,
    },
    MotionBlur {
        angle: f32,
        distance: f32,
    },
    /// Amount 1–100; centre as a fraction of the bounds (0.5, 0.5 = centre).
    RadialBlur {
        amount: f32,
        method: RadialMethod,
        #[serde(default)]
        quality: RadialQuality,
        center_x: f32,
        center_y: f32,
    },
    /// Radius px, threshold in levels.
    SurfaceBlur {
        radius: f32,
        threshold: f32,
    },
    /// Amount %, radius px, threshold levels.
    UnsharpMask {
        amount: f32,
        radius: f32,
        threshold: f32,
    },
    /// Amount %, radius px, noise reduction % (basic: Gaussian unsharp).
    SmartSharpen {
        amount: f32,
        radius: f32,
        reduce_noise: f32,
    },
    HighPass {
        radius: f32,
    },
    /// Amount 0–400 %.
    AddNoise {
        amount: f32,
        distribution: Distribution,
        monochromatic: bool,
        seed: u32,
    },
    Median {
        radius: f32,
    },
    DustAndScratches {
        radius: f32,
        threshold: f32,
    },
    /// Radius px; `preserve` is the dialog's Preserve (older params default to Squareness).
    Minimum {
        radius: f32,
        #[serde(default)]
        preserve: Preserve,
    },
    Maximum {
        radius: f32,
        #[serde(default)]
        preserve: Preserve,
    },
    Offset {
        horizontal: i32,
        vertical: i32,
        undefined: UndefinedAreas,
    },
    Mosaic {
        cell_size: f32,
    },
    /// Angle degrees, height px, amount %.
    Emboss {
        angle: f32,
        height: f32,
        amount: f32,
    },
    FindEdges,
    Solarize,
    Invert,
    Desaturate,
    /// Angle −999…999 degrees.
    Twirl {
        angle: f32,
    },
    /// Amount −100…100 %.
    Pinch {
        amount: f32,
    },
    /// Amount −100…100 %.
    Spherize {
        amount: f32,
        mode: SpherizeMode,
    },
    Wave {
        generators: u32,
        wavelength_min: f32,
        wavelength_max: f32,
        amplitude_min: f32,
        amplitude_max: f32,
        wave_type: WaveType,
        undefined: UndefinedAreas,
        seed: u32,
    },
    /// Amount −999…999 %.
    Ripple {
        amount: f32,
        size: RippleSize,
    },
    PolarCoordinates {
        mode: PolarMode,
    },

    // ---- Pixelate ----
    /// Max dot radius px (4–127); screen angles per channel in degrees.
    ColorHalftone {
        max_radius: f32,
        angles: [f32; 4],
    },
    /// Cell size px (3–300).
    Crystallize {
        cell_size: f32,
        seed: u32,
    },
    Facet,
    Fragment,
    Mezzotint {
        kind: MezzotintType,
        seed: u32,
    },
    /// Cell size px (3–300); `background` is straight sRGB RGBA.
    Pointillize {
        cell_size: f32,
        seed: u32,
        background: [f32; 4],
    },

    // ---- Stylize ----
    Diffuse {
        mode: DiffuseMode,
        seed: u32,
    },
    /// Size px (2–255), depth (1–255).
    Extrude {
        kind: ExtrudeType,
        size: f32,
        depth: f32,
        level_based: bool,
        solid_front: bool,
        mask_incomplete: bool,
        seed: u32,
    },
    /// Photoshop's Oil Paint units: stylization 0.1–10, cleanliness 0–10, scale 0.1–10,
    /// bristle detail 0–10, lighting angle degrees, shine 0–10.
    OilPaint {
        stylization: f32,
        cleanliness: f32,
        scale: f32,
        bristle_detail: f32,
        lighting: bool,
        angle: f32,
        shine: f32,
    },
    /// Tiles along the shorter side (1–99), max offset % (1–99).
    Tiles {
        count: u32,
        max_offset: f32,
        fill: TileFill,
        foreground: [f32; 4],
        background: [f32; 4],
        seed: u32,
    },
    /// Level 0–255; `upper` traces the side above the level.
    TraceContour {
        level: f32,
        upper: bool,
    },
    Wind {
        method: WindMethod,
        from_right: bool,
        seed: u32,
    },

    // ---- Distort ----
    /// Scales in % (−999…999; 100 % = 128 px at full map contrast). The map is
    /// supplied by the caller (another document, layer or file).
    Displace {
        horizontal: f32,
        vertical: f32,
        stretch: bool,
        undefined: UndefinedAreas,
        #[serde(skip)]
        map: Option<std::sync::Arc<Image>>,
    },
    /// Curve points `[t, offset]`: t 0–1 top→bottom, offset −1…1 of half the width.
    Shear {
        points: Vec<[f32; 2]>,
        undefined: UndefinedAreas,
    },
    /// Amount −100…100, ridges 0–20.
    ZigZag {
        amount: f32,
        ridges: f32,
        style: ZigZagStyle,
    },

    // ---- Render ----
    /// Variance 1–64, strength 1–64; colours straight sRGB RGBA.
    Fibers {
        variance: f32,
        strength: f32,
        seed: u32,
        foreground: [f32; 4],
        background: [f32; 4],
    },
    /// Brightness 10–300 %, centre as a fraction of the bounds.
    LensFlare {
        brightness: f32,
        center_x: f32,
        center_y: f32,
        lens: LensType,
    },
    /// Gloss/metallic/exposure/ambience −100…100; bump height 0–100.
    LightingEffects {
        lights: Vec<Light>,
        gloss: f32,
        metallic: f32,
        exposure: f32,
        ambience: f32,
        texture: TextureChannel,
        height: f32,
        white_is_high: bool,
    },
    /// Photographic relight: angle −180…180°, elevation 0…90°, intensity/ambient 0…100,
    /// warmth −100…100, softness 1…100. Intensity 0 is an identity copy.
    Relight {
        angle: f32,
        elevation: f32,
        intensity: f32,
        ambient: f32,
        warmth: f32,
        softness: f32,
    },

    // ---- Noise ----
    /// Strength 0–10, the rest 0–100 %.
    ReduceNoise {
        strength: f32,
        preserve_details: f32,
        reduce_color_noise: f32,
        sharpen_details: f32,
        remove_jpeg_artifact: bool,
    },

    // ---- Blur ----
    /// Radius 0.1–100 px, threshold 0.1–100 levels.
    SmartBlur {
        radius: f32,
        threshold: f32,
        quality: BlurQuality,
        mode: SmartBlurMode,
    },
    /// Iris radius 0–100 px, `blades` 3–8, curvature 0–100, rotation degrees;
    /// focal distance 0–255; specular brightness 0–100 / threshold 0–255; noise 0–100.
    LensBlur {
        radius: f32,
        blades: u32,
        curvature: f32,
        rotation: f32,
        depth: DepthSource,
        focal_distance: f32,
        invert_depth: bool,
        brightness: f32,
        threshold: f32,
        noise: f32,
        distribution: Distribution,
        monochromatic: bool,
        seed: u32,
        /// Depth for [`DepthSource::LayerMask`] (channel 0, document coordinates), supplied by the caller.
        #[serde(skip)]
        depth_map: Option<std::sync::Arc<Image>>,
    },
    /// Radius 5–1000 px.
    ShapeBlur {
        radius: f32,
        shape: BlurShape,
    },

    // ---- Blur Gallery ----
    /// Centre fractions, angle degrees, focus/transition as fractions of the bounds' shorter side.
    TiltShift {
        blur: f32,
        center_x: f32,
        center_y: f32,
        angle: f32,
        focus: f32,
        transition: f32,
    },
    IrisBlur {
        pins: Vec<IrisPin>,
    },
    FieldBlur {
        pins: Vec<FieldPin>,
    },
    SpinBlur {
        pins: Vec<SpinPin>,
    },
    PathBlur {
        paths: Vec<BlurPath>,
    },

    // ---- Other ----
    /// 5×5 kernel (row-major, centre = index 12), divided by `scale`, plus `offset` levels.
    Custom {
        kernel: Vec<f32>,
        scale: f32,
        offset: f32,
    },
    HsbHsl {
        input: HsbModel,
        output: HsbModel,
    },
    /// Removes `color` (straight sRGB RGBA, alpha ignored) by unmixing it from every pixel into
    /// transparency (see [`color_to_alpha`]). Thresholds 0–1: opacities at or below
    /// `transparency_threshold` become 0, at or above `opacity_threshold` 1, linear in between.
    ColorToAlpha {
        color: [f32; 4],
        transparency_threshold: f32,
        opacity_threshold: f32,
    },

    // ---- Video ----
    /// Removes the even (or odd) lines and rebuilds them by interpolation (or duplication).
    DeInterlace {
        eliminate_even: bool,
        interpolate: bool,
    },
    NtscColors,
    /// Filter Gallery: a stack of gallery effects applied in order.
    FilterGallery {
        effects: Vec<GalleryEffect>,
    },
}

impl FilterParams {
    /// Pixels read outside an output tile.
    pub fn halo(&self) -> Halo {
        let g = |sigma: f32| Halo::Radius((sigma.max(0.0) * 3.0).ceil() as i32 + 1);
        match self {
            FilterParams::GaussianBlur { radius } | FilterParams::HighPass { radius } => g(*radius),
            FilterParams::UnsharpMask { radius, .. } => Halo::Radius(sharpen::exact::halo(*radius)),
            FilterParams::SmartSharpen { radius, .. } => g(*radius),
            FilterParams::BoxBlur { radius }
            | FilterParams::SurfaceBlur { radius, .. }
            | FilterParams::Median { radius }
            | FilterParams::DustAndScratches { radius, .. }
            | FilterParams::Minimum { radius, .. }
            | FilterParams::Maximum { radius, .. } => {
                Halo::Radius(radius.max(0.0).ceil() as i32 + 1)
            }
            FilterParams::MotionBlur { distance, .. } => {
                Halo::Radius((distance.abs() / 2.0).ceil() as i32 + 2)
            }
            FilterParams::Mosaic { cell_size } => {
                Halo::Radius(cell_size.max(1.0).ceil() as i32 + 1)
            }
            FilterParams::Emboss { height, .. } => Halo::Radius(height.abs().ceil() as i32 + 2),
            FilterParams::FindEdges => Halo::Radius(1),
            FilterParams::AddNoise { .. }
            | FilterParams::Solarize
            | FilterParams::Invert
            | FilterParams::Desaturate => Halo::Radius(0),
            FilterParams::RadialBlur { .. }
            | FilterParams::Offset { .. }
            | FilterParams::Twirl { .. }
            | FilterParams::Pinch { .. }
            | FilterParams::Spherize { .. }
            | FilterParams::Wave { .. }
            | FilterParams::Ripple { .. }
            | FilterParams::PolarCoordinates { .. } => Halo::Bounds,
            other => halo_ext(other),
        }
    }

    /// [`Self::halo`] using the document/selection bounds so filters whose reach
    /// depends on size (Relight's shading blur) request the neighbourhood they actually read.
    pub fn halo_for(&self, bounds: Rect) -> Halo {
        match self {
            FilterParams::Relight {
                intensity,
                softness,
                ..
            } if *intensity != 0.0 => Halo::Radius(relight::halo_radius(*softness, bounds)),
            _ => self.halo(),
        }
    }

    /// Whether the filter's job is to make pixels transparent (Color to Alpha): it needs a layer
    /// that can hold transparency, so callers turn the Background into a normal layer first and
    /// refuse a layer whose transparency is locked instead of putting the old alpha back.
    pub fn makes_transparency(&self) -> bool {
        matches!(self, FilterParams::ColorToAlpha { .. })
    }

    /// Whether the filter moves pixels around the reference bounds (its
    /// output area is the bounds, not the layer's content grown by the halo).
    pub fn is_global(&self) -> bool {
        self.halo() == Halo::Bounds
    }

    /// Whether the filter renders new pixels without reading the layer's own (Fibers): it fills
    /// the reference bounds even where the layer is transparent, as Photoshop's does on an
    /// empty layer, while still reading no neighbourhood.
    pub fn renders_from_nothing(&self) -> bool {
        matches!(self, FilterParams::Fibers { .. })
    }

    /// Human-readable name.
    pub fn label(&self) -> &'static str {
        match self {
            FilterParams::GaussianBlur { .. } => "Gaussian Blur",
            FilterParams::BoxBlur { .. } => "Box Blur",
            FilterParams::MotionBlur { .. } => "Motion Blur",
            FilterParams::RadialBlur { .. } => "Radial Blur",
            FilterParams::SurfaceBlur { .. } => "Surface Blur",
            FilterParams::UnsharpMask { .. } => "Unsharp Mask",
            FilterParams::SmartSharpen { .. } => "Smart Sharpen",
            FilterParams::HighPass { .. } => "High Pass",
            FilterParams::AddNoise { .. } => "Add Noise",
            FilterParams::Median { .. } => "Median",
            FilterParams::DustAndScratches { .. } => "Dust & Scratches",
            FilterParams::Minimum { .. } => "Minimum",
            FilterParams::Maximum { .. } => "Maximum",
            FilterParams::Offset { .. } => "Offset",
            FilterParams::Mosaic { .. } => "Mosaic",
            FilterParams::Emboss { .. } => "Emboss",
            FilterParams::FindEdges => "Find Edges",
            FilterParams::Solarize => "Solarize",
            FilterParams::Invert => "Invert",
            FilterParams::Desaturate => "Desaturate",
            FilterParams::Twirl { .. } => "Twirl",
            FilterParams::Pinch { .. } => "Pinch",
            FilterParams::Spherize { .. } => "Spherize",
            FilterParams::Wave { .. } => "Wave",
            FilterParams::Ripple { .. } => "Ripple",
            FilterParams::PolarCoordinates { .. } => "Polar Coordinates",
            FilterParams::ColorHalftone { .. } => "Color Halftone",
            FilterParams::Crystallize { .. } => "Crystallize",
            FilterParams::Facet => "Facet",
            FilterParams::Fragment => "Fragment",
            FilterParams::Mezzotint { .. } => "Mezzotint",
            FilterParams::Pointillize { .. } => "Pointillize",
            FilterParams::Diffuse { .. } => "Diffuse",
            FilterParams::Extrude { .. } => "Extrude",
            FilterParams::OilPaint { .. } => "Oil Paint",
            FilterParams::Tiles { .. } => "Tiles",
            FilterParams::TraceContour { .. } => "Trace Contour",
            FilterParams::Wind { .. } => "Wind",
            FilterParams::Displace { .. } => "Displace",
            FilterParams::Shear { .. } => "Shear",
            FilterParams::ZigZag { .. } => "ZigZag",
            FilterParams::Fibers { .. } => "Fibers",
            FilterParams::LensFlare { .. } => "Lens Flare",
            FilterParams::LightingEffects { .. } => "Lighting Effects",
            FilterParams::Relight { .. } => "Relight",
            FilterParams::ReduceNoise { .. } => "Reduce Noise",
            FilterParams::SmartBlur { .. } => "Smart Blur",
            FilterParams::LensBlur { .. } => "Lens Blur",
            FilterParams::ShapeBlur { .. } => "Shape Blur",
            FilterParams::TiltShift { .. } => "Tilt-Shift",
            FilterParams::IrisBlur { .. } => "Iris Blur",
            FilterParams::FieldBlur { .. } => "Field Blur",
            FilterParams::SpinBlur { .. } => "Spin Blur",
            FilterParams::PathBlur { .. } => "Path Blur",
            FilterParams::Custom { .. } => "Custom",
            FilterParams::HsbHsl { .. } => "HSB/HSL",
            FilterParams::ColorToAlpha { .. } => "Color to Alpha",
            FilterParams::DeInterlace { .. } => "De-Interlace",
            FilterParams::NtscColors => "NTSC Colors",
            FilterParams::FilterGallery { effects } => match effects.as_slice() {
                [one] => one.filter.name(),
                _ => "Filter Gallery",
            },
        }
    }
}

/// Halos of the second filter batch.
fn halo_ext(p: &FilterParams) -> Halo {
    let r = |v: f32| Halo::Radius(v.max(0.0).ceil() as i32 + 1);
    match p {
        FilterParams::ColorHalftone { max_radius, .. } => r(max_radius * 3.0),
        FilterParams::Crystallize { cell_size, .. } => r(cell_size.max(1.0) * 3.0),
        FilterParams::Pointillize { cell_size, .. } => r(cell_size.max(1.0) * 2.0),
        FilterParams::Facet => r(3.0),
        FilterParams::Fragment => r(4.0),
        FilterParams::Mezzotint { .. }
        | FilterParams::HsbHsl { .. }
        | FilterParams::ColorToAlpha { .. }
        | FilterParams::NtscColors => Halo::Radius(0),
        FilterParams::Diffuse { .. } => r(1.0),
        FilterParams::Extrude { size, depth, .. } => {
            r(stylize2::extrude_reach(*depth) + size.clamp(2.0, 255.0) * 3.0 + 1.0)
        }
        FilterParams::OilPaint {
            stylization, scale, ..
        } => r(oil::reach(*stylization, *scale)),
        FilterParams::Tiles { .. } => Halo::Bounds,
        FilterParams::TraceContour { .. } => r(1.0),
        FilterParams::Wind { method, .. } => r(stylize2::wind_reach(*method)),
        FilterParams::Displace { .. }
        | FilterParams::Shear { .. }
        | FilterParams::ZigZag { .. }
        | FilterParams::SpinBlur { .. } => Halo::Bounds,
        // Generators and per-pixel renders only need the bounds geometry (always in the context).
        FilterParams::Fibers { .. } | FilterParams::LensFlare { .. } => Halo::Radius(0),
        FilterParams::LightingEffects { .. } => r(1.0),
        FilterParams::Relight { intensity, .. } if *intensity == 0.0 => Halo::Radius(0),
        FilterParams::Relight { .. } => Halo::Radius(relight::halo_radius_max()),
        FilterParams::ReduceNoise { .. } => r(denoise::reach()),
        FilterParams::SmartBlur { radius, .. } => r(*radius),
        FilterParams::LensBlur { radius, .. } => r(*radius),
        FilterParams::ShapeBlur { radius, .. } => r(shape_blur::radius(*radius)),
        FilterParams::TiltShift { blur, .. } => r(gallery::reach(*blur)),
        FilterParams::IrisBlur { pins } => r(gallery::reach(
            pins.iter().map(|p| p.blur).fold(0.0, f32::max),
        )),
        FilterParams::FieldBlur { pins } => r(gallery::reach(
            pins.iter().map(|p| p.blur).fold(0.0, f32::max),
        )),
        FilterParams::PathBlur { paths } => {
            r(paths.iter().map(|p| p.speed.abs()).fold(0.0, f32::max) / 2.0 + 1.0)
        }
        FilterParams::Custom { .. } => r(2.0),
        FilterParams::DeInterlace { .. } => r(1.0),
        FilterParams::FilterGallery { effects } => Halo::Radius(artistic_fx::reach(effects)),
        _ => Halo::Radius(0),
    }
}
