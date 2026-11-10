//! Pure-Rust Resolution-Independent Ken Burns Slideshow & Backdrop Compositor (#109).
//!
//! Provides:
//! - Resolution-independent layout geometry scaled proportionally to `min(viewport_w, viewport_h)`
//! - Automatic visual saliency / high-contrast point-of-interest detection (`detect_salient_focus`)
//! - Smooth Ken Burns pan-and-zoom trajectory interpolation (Linear, Smoothstep, Cosine, CubicBezier)
//! - High-performance backdrop rendering (`BlurredMirror`, `SolidColor`, `LinearGradient`),
//!   proportional drop shadows, and matted border frames.

use serde::{Deserialize, Serialize};
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Easing curve for Ken Burns pan-and-zoom trajectories.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum KenBurnsEasing {
    Linear,
    #[default]
    Smoothstep,
    Cosine,
    CubicBezier {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
}

impl KenBurnsEasing {
    pub fn evaluate(&self, t: f32) -> f32 {
        let u = t.clamp(0.0, 1.0);
        match *self {
            Self::Linear => u,
            Self::Smoothstep => u * u * (3.0 - 2.0 * u),
            Self::Cosine => 0.5 * (1.0 - (u * std::f32::consts::PI).cos()),
            Self::CubicBezier { x1, y1, x2, y2 } => {
                // Newton-Raphson solve for parameter s where x(s) = u
                let mut s = u;
                for _ in 0..8 {
                    let inv = 1.0 - s;
                    let x = 3.0 * inv * inv * s * x1 + 3.0 * inv * s * s * x2 + s * s * s;
                    let dx = 3.0 * inv * inv * x1
                        + 6.0 * inv * s * (x2 - x1)
                        + 3.0 * s * s * (1.0 - x2);
                    if dx.abs() < 1e-6 {
                        break;
                    }
                    s = (s - (x - u) / dx).clamp(0.0, 1.0);
                }
                let inv = 1.0 - s;
                3.0 * inv * inv * s * y1 + 3.0 * inv * s * s * y2 + s * s * s
            }
        }
    }
}

/// Normalized camera focus state `(center_x, center_y, zoom)` in normalized `[0.0, 1.0]` coordinates.
/// `zoom = 1.0` fits the whole photo; `zoom = 1.4` zooms in by 1.4x around `(center_x, center_y)`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct KenBurnsKeyframe {
    pub center_x: f32,
    pub center_y: f32,
    pub zoom: f32,
}

impl Default for KenBurnsKeyframe {
    fn default() -> Self {
        Self {
            center_x: 0.5,
            center_y: 0.5,
            zoom: 1.0,
        }
    }
}

/// Complete Ken Burns trajectory between a start keyframe and an end keyframe.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct KenBurnsTrajectory {
    pub start: KenBurnsKeyframe,
    pub end: KenBurnsKeyframe,
    pub easing: KenBurnsEasing,
}

impl KenBurnsTrajectory {
    /// Interpolate the trajectory at normalized progress `t` in `[0.0, 1.0]`.
    pub fn sample(&self, t: f32) -> KenBurnsKeyframe {
        let a = self.easing.evaluate(t);
        KenBurnsKeyframe {
            center_x: self.start.center_x + (self.end.center_x - self.start.center_x) * a,
            center_y: self.start.center_y + (self.end.center_y - self.start.center_y) * a,
            zoom: (self.start.zoom + (self.end.zoom - self.start.zoom) * a).max(1.0),
        }
    }

    /// Automatically generate a cinematic Ken Burns trajectory that pans and zooms toward
    /// the highest-saliency region (edges, contrast, and warm skin-tone regions) of `photo`.
    pub fn auto_from_photo(photo: &Pixmap, zoom_amount: f32) -> Self {
        let (fx, fy) = detect_salient_focus(photo);
        let target_zoom = zoom_amount.clamp(1.05, 2.5);
        Self {
            start: KenBurnsKeyframe {
                center_x: 0.5,
                center_y: 0.5,
                zoom: 1.0,
            },
            end: KenBurnsKeyframe {
                center_x: fx,
                center_y: fy,
                zoom: target_zoom,
            },
            easing: KenBurnsEasing::Smoothstep,
        }
    }
}

/// Detect the primary point of interest `(norm_x, norm_y)` in `[0.15, 0.85]` using local
/// Sobel gradient energy combined with skin-tone / warm-subject chromaticity weighting.
pub fn detect_salient_focus(photo: &Pixmap) -> (f32, f32) {
    let w = photo.width() as i32;
    let h = photo.height() as i32;
    if w < 4 || h < 4 {
        return (0.5, 0.5);
    }

    let luma_at = |x: i32, y: i32| -> f32 {
        let cx = x.clamp(0, w - 1) as u32;
        let cy = y.clamp(0, h - 1) as u32;
        let p = photo.pixel(cx, cy).unwrap_or(PremultipliedColorU8::TRANSPARENT);
        (0.299 * p.red() as f32 + 0.587 * p.green() as f32 + 0.114 * p.blue() as f32) / 255.0
    };

    let mut sum_wx = 0.0f64;
    let mut sum_wy = 0.0f64;
    let mut sum_w = 0.0f64;

    let step = ((w.min(h) / 64).max(1)) as usize;
    for y in (1..(h - 1)).step_by(step) {
        for x in (1..(w - 1)).step_by(step) {
            let gx = luma_at(x + 1, y) - luma_at(x - 1, y);
            let gy = luma_at(x, y + 1) - luma_at(x, y - 1);
            let grad = (gx * gx + gy * gy).sqrt();

            let p = photo
                .pixel(x as u32, y as u32)
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
            let r = p.red() as f32;
            let g = p.green() as f32;
            let b = p.blue() as f32;
            // Boost regions with warm/skin-tone or high saturation
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);
            let sat = if max_c > 80.0 {
                ((max_c - min_c) / max_c) * (max_c / 255.0)
            } else {
                0.0
            };

            let weight = (grad * 3.0 + sat * 0.8) as f64;
            if grad > 0.02 || sat > 0.15 {
                sum_wx += (x as f64 / w as f64) * weight;
                sum_wy += (y as f64 / h as f64) * weight;
                sum_w += weight;
            }
        }
    }

    if sum_w > 1e-5 {
        let nx = (sum_wx / sum_w) as f32;
        let ny = (sum_wy / sum_w) as f32;
        (nx.clamp(0.15, 0.85), ny.clamp(0.15, 0.85))
    } else {
        (0.5, 0.5)
    }
}

/// Backdrop style behind a framed slideshow photograph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SlideshowBackdrop {
    /// Cover-scaled, blurred, darkened mirror of the slide photo itself.
    BlurredMirror {
        /// Blur radius as a fraction of viewport shorter dimension (e.g. `0.03`).
        blur_fraction: f32,
        /// Brightness multiplier in `0.0..=1.0` (e.g. `0.45`).
        brightness: f32,
    },
    /// Solid RGBA color fill.
    SolidColor([u8; 4]),
    /// Vertical linear gradient between `top` and `bottom` RGBA colors.
    LinearGradient { top: [u8; 4], bottom: [u8; 4] },
}

impl Default for SlideshowBackdrop {
    fn default() -> Self {
        Self::BlurredMirror {
            blur_fraction: 0.025,
            brightness: 0.45,
        }
    }
}

/// Resolution-independent layout configuration where all spatial metrics are expressed
/// as fractions of the viewport's shorter dimension (`min(viewport_w, viewport_h)`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideshowLayoutConfig {
    /// Outer margin around the framed photo as a fraction of `min(w, h)` (e.g. `0.08` = 8%).
    pub margin_fraction: f32,
    /// Border stroke width around the photo as a fraction of `min(w, h)` (e.g. `0.008`).
    pub border_fraction: f32,
    /// Border RGBA color (e.g. `[255, 255, 255, 255]`).
    pub border_color: [u8; 4],
    /// Drop shadow blur radius as a fraction of `min(w, h)` (e.g. `0.025`).
    pub shadow_softness_fraction: f32,
    /// Drop shadow downward offset as a fraction of `min(w, h)` (e.g. `0.015`).
    pub shadow_elevation_fraction: f32,
    /// Drop shadow opacity (`0.0..=1.0`).
    pub shadow_opacity: f32,
    /// Backdrop style.
    pub backdrop: SlideshowBackdrop,
}

impl Default for SlideshowLayoutConfig {
    fn default() -> Self {
        Self {
            margin_fraction: 0.08,
            border_fraction: 0.006,
            border_color: [255, 255, 255, 255],
            shadow_softness_fraction: 0.025,
            shadow_elevation_fraction: 0.012,
            shadow_opacity: 0.55,
            backdrop: SlideshowBackdrop::default(),
        }
    }
}

/// Concrete pixel-space layout geometry computed from `SlideshowLayoutConfig` for a given viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedSlideGeometry {
    pub viewport_w: u32,
    pub viewport_h: u32,
    pub short_side: f32,
    pub margin_px: f32,
    pub border_px: f32,
    pub shadow_softness_px: f32,
    pub shadow_elevation_px: f32,
    /// Inner photo rectangle `(x, y, width, height)` in viewport pixel coordinates.
    pub photo_rect: [f32; 4],
}

impl SlideshowLayoutConfig {
    /// Resolve resolution-independent fractions to exact viewport pixel coordinates for a photo
    /// of aspect ratio `photo_w / photo_h`.
    pub fn resolve_geometry(
        &self,
        viewport_w: u32,
        viewport_h: u32,
        photo_w: u32,
        photo_h: u32,
    ) -> ResolvedSlideGeometry {
        let vw = viewport_w.max(1) as f32;
        let vh = viewport_h.max(1) as f32;
        let short_side = vw.min(vh);

        let margin_px = (self.margin_fraction.clamp(0.0, 0.4) * short_side).round();
        let border_px = (self.border_fraction.clamp(0.0, 0.1) * short_side).round();
        let shadow_softness_px = self.shadow_softness_fraction.clamp(0.0, 0.15) * short_side;
        let shadow_elevation_px = self.shadow_elevation_fraction.clamp(0.0, 0.15) * short_side;

        let avail_w = (vw - 2.0 * (margin_px + border_px)).max(4.0);
        let avail_h = (vh - 2.0 * (margin_px + border_px)).max(4.0);

        let photo_aspect = (photo_w.max(1) as f32) / (photo_h.max(1) as f32);
        let avail_aspect = avail_w / avail_h;

        let (fit_w, fit_h) = if photo_aspect > avail_aspect {
            (avail_w, (avail_w / photo_aspect).round().max(1.0))
        } else {
            ((avail_h * photo_aspect).round().max(1.0), avail_h)
        };

        let rx = ((vw - fit_w) * 0.5).round();
        let ry = ((vh - fit_h) * 0.5).round();

        ResolvedSlideGeometry {
            viewport_w,
            viewport_h,
            short_side,
            margin_px,
            border_px,
            shadow_softness_px,
            shadow_elevation_px,
            photo_rect: [rx, ry, fit_w, fit_h],
        }
    }
}

/// Bilinearly sample `src` at normalized coordinates `(u, v)` in `[0.0, 1.0]`.
#[inline]
fn sample_normalized_bilinear(src: &Pixmap, u: f32, v: f32) -> [f32; 4] {
    let w = src.width();
    let h = src.height();
    let sx = (u.clamp(0.0, 1.0) * (w as f32 - 1.0)).clamp(0.0, w as f32 - 1.0);
    let sy = (v.clamp(0.0, 1.0) * (h as f32 - 1.0)).clamp(0.0, h as f32 - 1.0);

    let x0 = sx.floor() as u32;
    let y0 = sy.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = sx - x0 as f32;
    let fy = sy - y0 as f32;

    let get = |x: u32, y: u32| -> [f32; 4] {
        let p = src.pixel(x, y).unwrap_or(PremultipliedColorU8::TRANSPARENT);
        [
            p.red() as f32,
            p.green() as f32,
            p.blue() as f32,
            p.alpha() as f32,
        ]
    };

    let p00 = get(x0, y0);
    let p10 = get(x1, y0);
    let p01 = get(x0, y1);
    let p11 = get(x1, y1);

    let mut out = [0.0f32; 4];
    for c in 0..4 {
        let top = p00[c] * (1.0 - fx) + p10[c] * fx;
        let bot = p01[c] * (1.0 - fx) + p11[c] * fx;
        out[c] = top * (1.0 - fy) + bot * fy;
    }
    out
}

/// Fast separable box blur in-place on a `Pixmap`.
fn box_blur_pixmap(pixmap: &mut Pixmap, radius: u32) {
    if radius == 0 {
        return;
    }
    let w = pixmap.width() as i32;
    let h = pixmap.height() as i32;
    let r = (radius as i32).min(w.min(h) / 2).max(1);
    let diam = (2 * r + 1) as f32;

    let mut temp = vec![[0.0f32; 4]; (w * h) as usize];
    let src = pixmap.pixels();

    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for k in -r..=r {
                let sx = (x + k).clamp(0, w - 1);
                let p = src[(y * w + sx) as usize];
                acc[0] += p.red() as f32;
                acc[1] += p.green() as f32;
                acc[2] += p.blue() as f32;
                acc[3] += p.alpha() as f32;
            }
            temp[(y * w + x) as usize] = [
                acc[0] / diam,
                acc[1] / diam,
                acc[2] / diam,
                acc[3] / diam,
            ];
        }
    }

    let dst = pixmap.pixels_mut();
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for k in -r..=r {
                let sy = (y + k).clamp(0, h - 1);
                let p = temp[(sy * w + x) as usize];
                for c in 0..4 {
                    acc[c] += p[c];
                }
            }
            let a = (acc[3] / diam).clamp(0.0, 255.0).round() as u8;
            let pr = (acc[0] / diam).clamp(0.0, a as f32).round() as u8;
            let pg = (acc[1] / diam).clamp(0.0, a as f32).round() as u8;
            let pb = (acc[2] / diam).clamp(0.0, a as f32).round() as u8;
            dst[(y * w + x) as usize] = PremultipliedColorU8::from_rgba(pr, pg, pb, a)
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        }
    }
}

/// Render a single frame of a Ken Burns slideshow onto a viewport of `(viewport_w, viewport_h)`.
pub fn render_slideshow_frame(
    photo: &Pixmap,
    viewport_w: u32,
    viewport_h: u32,
    config: &SlideshowLayoutConfig,
    trajectory: &KenBurnsTrajectory,
    progress: f32,
) -> Pixmap {
    let mut canvas = Pixmap::new(viewport_w.max(1), viewport_h.max(1)).unwrap();
    let geom = config.resolve_geometry(viewport_w, viewport_h, photo.width(), photo.height());

    // 1. Render Backdrop
    match config.backdrop {
        SlideshowBackdrop::SolidColor([r, g, b, a]) => {
            let af = a as f32 / 255.0;
            let px = PremultipliedColorU8::from_rgba(
                (r as f32 * af).round() as u8,
                (g as f32 * af).round() as u8,
                (b as f32 * af).round() as u8,
                a,
            )
            .unwrap_or(PremultipliedColorU8::TRANSPARENT);
            canvas.pixels_mut().fill(px);
        }
        SlideshowBackdrop::LinearGradient { top, bottom } => {
            for y in 0..viewport_h {
                let t = y as f32 / (viewport_h.max(2) - 1) as f32;
                let r = top[0] as f32 * (1.0 - t) + bottom[0] as f32 * t;
                let g = top[1] as f32 * (1.0 - t) + bottom[1] as f32 * t;
                let b = top[2] as f32 * (1.0 - t) + bottom[2] as f32 * t;
                let a = top[3] as f32 * (1.0 - t) + bottom[3] as f32 * t;
                let au8 = a.round().clamp(0.0, 255.0) as u8;
                let af = au8 as f32 / 255.0;
                let px = PremultipliedColorU8::from_rgba(
                    (r * af).round().min(au8 as f32) as u8,
                    (g * af).round().min(au8 as f32) as u8,
                    (b * af).round().min(au8 as f32) as u8,
                    au8,
                )
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
                let row_start = (y * viewport_w) as usize;
                canvas.pixels_mut()[row_start..row_start + viewport_w as usize].fill(px);
            }
        }
        SlideshowBackdrop::BlurredMirror {
            blur_fraction,
            brightness,
        } => {
            let br = brightness.clamp(0.0, 1.0);
            for y in 0..viewport_h {
                let v = y as f32 / viewport_h.max(1) as f32;
                for x in 0..viewport_w {
                    let u = x as f32 / viewport_w.max(1) as f32;
                    let [r, g, b, _] = sample_normalized_bilinear(photo, u, v);
                    let px = PremultipliedColorU8::from_rgba(
                        (r * br).clamp(0.0, 255.0).round() as u8,
                        (g * br).clamp(0.0, 255.0).round() as u8,
                        (b * br).clamp(0.0, 255.0).round() as u8,
                        255,
                    )
                    .unwrap();
                    canvas.pixels_mut()[(y * viewport_w + x) as usize] = px;
                }
            }
            let blur_r = (blur_fraction.clamp(0.0, 0.1) * geom.short_side).round() as u32;
            if blur_r > 0 {
                box_blur_pixmap(&mut canvas, blur_r);
                box_blur_pixmap(&mut canvas, blur_r);
            }
        }
    }

    let [px_x, px_y, px_w, px_h] = geom.photo_rect;
    let border = geom.border_px;
    let outer_x0 = px_x - border;
    let outer_y0 = px_y - border;
    let outer_x1 = px_x + px_w + border;
    let outer_y1 = px_y + px_h + border;

    // 2. Render Soft Drop Shadow behind outer frame
    if config.shadow_opacity > 0.001 && geom.shadow_softness_px > 0.5 {
        let soft = geom.shadow_softness_px.max(1.0);
        let sh_x0 = outer_x0;
        let sh_y0 = outer_y0 + geom.shadow_elevation_px;
        let sh_x1 = outer_x1;
        let sh_y1 = outer_y1 + geom.shadow_elevation_px;

        let min_x = (sh_x0 - soft * 2.0).floor().max(0.0) as u32;
        let max_x = (sh_x1 + soft * 2.0).ceil().min(viewport_w as f32) as u32;
        let min_y = (sh_y0 - soft * 2.0).floor().max(0.0) as u32;
        let max_y = (sh_y1 + soft * 2.0).ceil().min(viewport_h as f32) as u32;

        for y in min_y..max_y {
            let fy = y as f32 + 0.5;
            let dy = if fy < sh_y0 {
                sh_y0 - fy
            } else if fy > sh_y1 {
                fy - sh_y1
            } else {
                0.0
            };
            for x in min_x..max_x {
                let fx = x as f32 + 0.5;
                let dx = if fx < sh_x0 {
                    sh_x0 - fx
                } else if fx > sh_x1 {
                    fx - sh_x1
                } else {
                    0.0
                };
                let dist = (dx * dx + dy * dy).sqrt();
                let falloff = (-dist * dist / (2.0 * soft * soft)).exp();
                let shadow_alpha = (config.shadow_opacity * falloff).clamp(0.0, 1.0);
                if shadow_alpha > 0.005 {
                    let idx = (y * viewport_w + x) as usize;
                    let dst = canvas.pixels()[idx];
                    let inv = 1.0 - shadow_alpha;
                    canvas.pixels_mut()[idx] = PremultipliedColorU8::from_rgba(
                        (dst.red() as f32 * inv).round() as u8,
                        (dst.green() as f32 * inv).round() as u8,
                        (dst.blue() as f32 * inv).round() as u8,
                        dst.alpha(),
                    )
                    .unwrap();
                }
            }
        }
    }

    // 3. Render Border Frame
    if border > 0.0 {
        let [br, bg, bb, ba] = config.border_color;
        let baf = ba as f32 / 255.0;
        let border_px = PremultipliedColorU8::from_rgba(
            (br as f32 * baf).round() as u8,
            (bg as f32 * baf).round() as u8,
            (bb as f32 * baf).round() as u8,
            ba,
        )
        .unwrap();

        let x_start = outer_x0.round().max(0.0) as u32;
        let x_end = outer_x1.round().min(viewport_w as f32) as u32;
        let y_start = outer_y0.round().max(0.0) as u32;
        let y_end = outer_y1.round().min(viewport_h as f32) as u32;

        for y in y_start..y_end {
            for x in x_start..x_end {
                canvas.pixels_mut()[(y * viewport_w + x) as usize] = border_px;
            }
        }
    }

    // 4. Sample Ken Burns Trajectory & Render Cropped Photo
    let kf = trajectory.sample(progress);
    let half_span = 0.5 / kf.zoom.max(1.0);
    let cx = kf.center_x.clamp(half_span, 1.0 - half_span);
    let cy = kf.center_y.clamp(half_span, 1.0 - half_span);
    let u0 = cx - half_span;
    let v0 = cy - half_span;
    let span = 2.0 * half_span;

    let ix0 = px_x.round().max(0.0) as u32;
    let ix1 = (px_x + px_w).round().min(viewport_w as f32) as u32;
    let iy0 = px_y.round().max(0.0) as u32;
    let iy1 = (px_y + px_h).round().min(viewport_h as f32) as u32;

    for y in iy0..iy1 {
        let ny = (y as f32 - px_y) / px_h.max(1.0);
        let v = v0 + ny * span;
        for x in ix0..ix1 {
            let nx = (x as f32 - px_x) / px_w.max(1.0);
            let u = u0 + nx * span;
            let [r, g, b, a] = sample_normalized_bilinear(photo, u, v);
            let au8 = a.round().clamp(0.0, 255.0) as u8;
            canvas.pixels_mut()[(y * viewport_w + x) as usize] = PremultipliedColorU8::from_rgba(
                r.round().clamp(0.0, au8 as f32) as u8,
                g.round().clamp(0.0, au8 as f32) as u8,
                b.round().clamp(0.0, au8 as f32) as u8,
                au8,
            )
            .unwrap();
        }
    }

    canvas
}
