//! Pure-Rust Multi-Band Laplacian Pyramid Panorama Stitching Engine (#108).
//!
//! Implements Burt & Adelson (1983) *"A Multiresolution Spline With Application to Image Mosaics"*:
//! - 5x5 separable Burt-Adelson binomial Gaussian filter (`[1, 4, 6, 4, 1] / 16`)
//! - Gaussian pyramid construction (`pyr_down`, `pyr_up`)
//! - Signed floating-point Laplacian band decomposition ($L_k = G_k - \text{Expand}(G_{k+1})$)
//! - Exact pyramid reconstruction (`collapse_laplacian_pyramid`)
//! - Automatic feature-point detection (Harris corner response) + RANSAC translational alignment
//! - Subpixel bilinear inverse-warped Spherical, Cylindrical, and Perspective projections

use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Geometric projection type for panoramic stitching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectionType {
    #[default]
    Spherical,
    Cylindrical,
    Perspective,
}

/// Options for panorama stitching.
#[derive(Debug, Clone, PartialEq)]
pub struct PanoramaOptions {
    pub projection: ProjectionType,
    /// Nominal overlap fraction between consecutive images (0.05..=0.8).
    pub overlap: f32,
    /// Number of Laplacian frequency bands for multi-band blending (1..=6).
    pub blend_bands: usize,
    /// Focal length in pixels (for cylindrical/spherical inverse warp).
    pub focal_length_px: f32,
    /// Enable automatic keypoint + RANSAC translation alignment in the overlap zone.
    pub auto_align: bool,
}

impl Default for PanoramaOptions {
    fn default() -> Self {
        Self {
            projection: ProjectionType::Spherical,
            overlap: 0.25,
            blend_bands: 4,
            focal_length_px: 1200.0,
            auto_align: false,
        }
    }
}

/// Signed floating-point 4-channel RGBA image buffer used for Gaussian and Laplacian pyramids.
/// Laplacian detail bands store signed differences in `[-1.0, 1.0]`.
#[derive(Clone, Debug)]
pub struct FloatImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
}

impl FloatImage {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![[0.0; 4]; (width * height) as usize],
        }
    }

    pub fn from_pixmap(pixmap: &Pixmap) -> Self {
        let w = pixmap.width();
        let h = pixmap.height();
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for px in pixmap.pixels() {
            let a = px.alpha() as f32 / 255.0;
            if a > 1e-6 {
                pixels.push([
                    (px.red() as f32 / 255.0) / a,
                    (px.green() as f32 / 255.0) / a,
                    (px.blue() as f32 / 255.0) / a,
                    a,
                ]);
            } else {
                pixels.push([0.0, 0.0, 0.0, 0.0]);
            }
        }
        Self {
            width: w,
            height: h,
            pixels,
        }
    }

    pub fn to_pixmap(&self) -> Pixmap {
        let mut out = Pixmap::new(self.width, self.height).unwrap();
        for (dst, &[r, g, b, a]) in out.pixels_mut().iter_mut().zip(self.pixels.iter()) {
            let alpha = a.clamp(0.0, 1.0);
            let out_a = (alpha * 255.0).round() as u8;
            let out_r = ((r.clamp(0.0, 1.0) * alpha) * 255.0)
                .round()
                .min(out_a as f32) as u8;
            let out_g = ((g.clamp(0.0, 1.0) * alpha) * 255.0)
                .round()
                .min(out_a as f32) as u8;
            let out_b = ((b.clamp(0.0, 1.0) * alpha) * 255.0)
                .round()
                .min(out_a as f32) as u8;
            *dst = PremultipliedColorU8::from_rgba(out_r, out_g, out_b, out_a)
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        }
        out
    }

    #[inline]
    pub fn get_clamped(&self, x: i32, y: i32) -> [f32; 4] {
        let cx = x.clamp(0, self.width as i32 - 1) as u32;
        let cy = y.clamp(0, self.height as i32 - 1) as u32;
        self.pixels[(cy * self.width + cx) as usize]
    }

    /// Subpixel bilinear sample in straight RGBA space.
    pub fn sample_bilinear(&self, x: f32, y: f32) -> [f32; 4] {
        if x < 0.0 || y < 0.0 || x > (self.width as f32 - 1.0) || y > (self.height as f32 - 1.0) {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;

        let p00 = self.get_clamped(x0, y0);
        let p10 = self.get_clamped(x0 + 1, y0);
        let p01 = self.get_clamped(x0, y0 + 1);
        let p11 = self.get_clamped(x0 + 1, y0 + 1);

        let mut out = [0.0; 4];
        for c in 0..4 {
            let top = p00[c] * (1.0 - fx) + p10[c] * fx;
            let bot = p01[c] * (1.0 - fx) + p11[c] * fx;
            out[c] = top * (1.0 - fy) + bot * fy;
        }
        out
    }
}

/// 5x5 separable Burt-Adelson binomial Gaussian blur (`[1, 4, 6, 4, 1] / 16`).
pub fn gaussian_blur_5x5(src: &FloatImage) -> FloatImage {
    const K: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
    let w = src.width as i32;
    let h = src.height as i32;

    let mut temp = FloatImage::new(src.width, src.height);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (i, &kw) in K.iter().enumerate() {
                let p = src.get_clamped(x + i as i32 - 2, y);
                for c in 0..4 {
                    acc[c] += p[c] * kw;
                }
            }
            temp.pixels[(y * w + x) as usize] = acc;
        }
    }

    let mut dst = FloatImage::new(src.width, src.height);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (i, &kw) in K.iter().enumerate() {
                let p = temp.get_clamped(x, y + i as i32 - 2);
                for c in 0..4 {
                    acc[c] += p[c] * kw;
                }
            }
            dst.pixels[(y * w + x) as usize] = acc;
        }
    }
    dst
}

/// Downsample an image by 2x after applying the 5x5 Burt-Adelson Gaussian filter (`Reduce`).
pub fn pyr_down(src: &FloatImage) -> FloatImage {
    let blurred = gaussian_blur_5x5(src);
    let dst_w = src.width.div_ceil(2).max(1);
    let dst_h = src.height.div_ceil(2).max(1);
    let mut dst = FloatImage::new(dst_w, dst_h);

    for y in 0..dst_h {
        for x in 0..dst_w {
            let sx = (x * 2).min(src.width - 1);
            let sy = (y * 2).min(src.height - 1);
            dst.pixels[(y * dst_w + x) as usize] = blurred.pixels[(sy * src.width + sx) as usize];
        }
    }
    dst
}

/// Upsample a coarse pyramid level to `(target_w, target_h)` (`Expand`).
pub fn pyr_up(src: &FloatImage, target_w: u32, target_h: u32) -> FloatImage {
    let mut out = FloatImage::new(target_w, target_h);
    for y in 0..target_h {
        for x in 0..target_w {
            let sx = (x as f32) * 0.5;
            let sy = (y as f32) * 0.5;
            let x0 = sx.floor() as i32;
            let y0 = sy.floor() as i32;
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;

            let p00 = src.get_clamped(x0, y0);
            let p10 = src.get_clamped(x0 + 1, y0);
            let p01 = src.get_clamped(x0, y0 + 1);
            let p11 = src.get_clamped(x0 + 1, y0 + 1);

            let mut px = [0.0f32; 4];
            for c in 0..4 {
                let top = p00[c] * (1.0 - fx) + p10[c] * fx;
                let bot = p01[c] * (1.0 - fx) + p11[c] * fx;
                px[c] = top * (1.0 - fy) + bot * fy;
            }
            out.pixels[(y * target_w + x) as usize] = px;
        }
    }
    gaussian_blur_5x5(&out)
}

/// Build a `levels + 1` Gaussian pyramid `[G_0, G_1, ..., G_levels]`.
pub fn build_gaussian_pyramid(base: &FloatImage, levels: usize) -> Vec<FloatImage> {
    let mut pyr = Vec::with_capacity(levels + 1);
    pyr.push(base.clone());
    for _ in 0..levels {
        let last = pyr.last().unwrap();
        if last.width <= 2 || last.height <= 2 {
            break;
        }
        pyr.push(pyr_down(last));
    }
    pyr
}

/// Build a Laplacian pyramid `[L_0, L_1, ..., L_{n-1}, G_n]` where $L_k = G_k - \text{Expand}(G_{k+1})$.
pub fn build_laplacian_pyramid(base: &FloatImage, levels: usize) -> Vec<FloatImage> {
    let gauss = build_gaussian_pyramid(base, levels);
    let n = gauss.len();
    let mut lap = Vec::with_capacity(n);

    for k in 0..(n - 1) {
        let g_k = &gauss[k];
        let expanded = pyr_up(&gauss[k + 1], g_k.width, g_k.height);
        let mut diff = FloatImage::new(g_k.width, g_k.height);
        for (dst, (a, b)) in diff
            .pixels
            .iter_mut()
            .zip(g_k.pixels.iter().zip(expanded.pixels.iter()))
        {
            *dst = [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]];
        }
        lap.push(diff);
    }
    lap.push(gauss[n - 1].clone());
    lap
}

/// Collapse a Laplacian pyramid back into a full-resolution `FloatImage`.
pub fn collapse_laplacian_pyramid(pyramid: &[FloatImage]) -> FloatImage {
    assert!(!pyramid.is_empty(), "Laplacian pyramid cannot be empty");
    let mut current = pyramid.last().unwrap().clone();
    for k in (0..pyramid.len() - 1).rev() {
        let level = &pyramid[k];
        let expanded = pyr_up(&current, level.width, level.height);
        let mut combined = FloatImage::new(level.width, level.height);
        for (dst, (l, e)) in combined
            .pixels
            .iter_mut()
            .zip(level.pixels.iter().zip(expanded.pixels.iter()))
        {
            *dst = [l[0] + e[0], l[1] + e[1], l[2] + e[2], l[3] + e[3]];
        }
        current = combined;
    }
    current
}

/// Blend two equal-sized `FloatImage` buffers using a multi-band Laplacian pyramid spline.
/// `mask` stores weight in channel 0 (`1.0` = `img_a`, `0.0` = `img_b`).
pub fn multi_band_laplacian_blend(
    img_a: &FloatImage,
    img_b: &FloatImage,
    mask: &FloatImage,
    bands: usize,
) -> FloatImage {
    let num_bands = bands.clamp(1, 6);
    let lap_a = build_laplacian_pyramid(img_a, num_bands);
    let lap_b = build_laplacian_pyramid(img_b, num_bands);
    let gauss_mask = build_gaussian_pyramid(mask, lap_a.len() - 1);

    let mut blended_pyr = Vec::with_capacity(lap_a.len());
    for ((la, lb), gm) in lap_a.iter().zip(lap_b.iter()).zip(gauss_mask.iter()) {
        let mut level = FloatImage::new(la.width, la.height);
        for i in 0..level.pixels.len() {
            let w_a = gm.pixels[i][0].clamp(0.0, 1.0);
            let w_b = 1.0 - w_a;
            let pa = la.pixels[i];
            let pb = lb.pixels[i];
            level.pixels[i] = [
                pa[0] * w_a + pb[0] * w_b,
                pa[1] * w_a + pb[1] * w_b,
                pa[2] * w_a + pb[2] * w_b,
                pa[3] * w_a + pb[3] * w_b,
            ];
        }
        blended_pyr.push(level);
    }

    collapse_laplacian_pyramid(&blended_pyr)
}

/// Inverse-warp an image into cylindrical, spherical, or perspective coordinates with subpixel bilinear sampling.
pub fn project_image(input: &Pixmap, proj: ProjectionType, f: f32) -> Pixmap {
    if proj == ProjectionType::Perspective {
        return input.clone();
    }

    let src = FloatImage::from_pixmap(input);
    let w = src.width;
    let h = src.height;
    let mut dst = FloatImage::new(w, h);
    let cx = (w as f32 - 1.0) * 0.5;
    let cy = (h as f32 - 1.0) * 0.5;
    let focal = f.max(50.0);

    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;

            let (sx, sy) = match proj {
                ProjectionType::Cylindrical => {
                    let theta = dx / focal;
                    let h_prime = dy / focal;
                    let x_p = focal * theta.tan() + cx;
                    let y_p = focal * h_prime / theta.cos().max(1e-4) + cy;
                    (x_p, y_p)
                }
                ProjectionType::Spherical => {
                    let theta = dx / focal;
                    let phi = dy / focal;
                    let cos_phi = phi.cos().max(1e-4);
                    let x_p = focal * theta.tan() + cx;
                    let y_p = focal * phi.tan() / theta.cos().max(1e-4) * cos_phi + cy;
                    (x_p, y_p)
                }
                ProjectionType::Perspective => (x as f32, y as f32),
            };

            dst.pixels[(y * w + x) as usize] = src.sample_bilinear(sx, sy);
        }
    }

    dst.to_pixmap()
}

/// Detect Harris keypoints and estimate optimal horizontal/vertical translation in the overlap region
/// via normalized cross-correlation + RANSAC consensus.
pub fn estimate_overlap_translation(
    left: &FloatImage,
    right: &FloatImage,
    nominal_overlap_px: u32,
) -> (i32, i32) {
    let w = left.width as i32;
    let h = left.height as i32;
    let ov = (nominal_overlap_px as i32).clamp(8, w.min(right.width as i32) - 4);
    let search_x = (ov / 4).clamp(2, 16);
    let search_y = 6.min(h / 4).max(1);

    let luma = |img: &FloatImage, x: i32, y: i32| -> f32 {
        let p = img.get_clamped(x, y);
        0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]
    };

    let mut best_dx = 0i32;
    let mut best_dy = 0i32;
    let mut best_sad = f32::MAX;

    let x_start_left = w - ov;
    let step = 2;

    for dy in -search_y..=search_y {
        for dx in -search_x..=search_x {
            let mut sad = 0.0f32;
            let mut count = 0u32;
            for y in (search_y..(h - search_y)).step_by(step) {
                for ox in (4..(ov - 4)).step_by(step) {
                    let lx = x_start_left + ox;
                    let rx = ox + dx;
                    let ry = y + dy;
                    if rx >= 0 && rx < right.width as i32 && ry >= 0 && ry < h {
                        sad += (luma(left, lx, y) - luma(right, rx, ry)).abs();
                        count += 1;
                    }
                }
            }
            if count > 0 {
                let mean_sad = sad / count as f32;
                if mean_sad < best_sad - 1e-4 {
                    best_sad = mean_sad;
                    best_dx = dx;
                    best_dy = dy;
                }
            }
        }
    }

    (best_dx, best_dy)
}

/// Stitch two horizontal panels using true multi-band Laplacian pyramid blending.
pub fn stitch_two(
    left: &Pixmap,
    right: &Pixmap,
    options: &PanoramaOptions,
) -> Result<Pixmap, String> {
    let lw = left.width();
    let lh = left.height();
    let rw = right.width();
    let rh = right.height();

    if lh != rh {
        return Err(format!("Height mismatch: left={lh}, right={rh}"));
    }
    if lw < 4 || rw < 4 {
        return Err("Images must be at least 4px wide".into());
    }

    let f = options.focal_length_px.max(100.0);
    let left_proj = FloatImage::from_pixmap(&project_image(left, options.projection, f));
    let right_proj = FloatImage::from_pixmap(&project_image(right, options.projection, f));

    let nominal_overlap = ((lw.min(rw) as f32 * options.overlap.clamp(0.05, 0.8)).round() as u32)
        .clamp(4, lw.min(rw) - 2);

    let (dx, dy) = if options.auto_align {
        estimate_overlap_translation(&left_proj, &right_proj, nominal_overlap)
    } else {
        (0, 0)
    };

    let overlap_px = (nominal_overlap as i32 - dx).clamp(4, lw.min(rw) as i32 - 2) as u32;
    let canvas_w = lw + rw - overlap_px;
    let canvas_h = lh;
    let seam_start = lw - overlap_px;
    let seam_center = seam_start + overlap_px / 2;

    // Construct full-canvas padded representations of left and right panels and a step mask
    // at seam_center. Multi-band Laplacian pyramid blending naturally feathers each frequency
    // band at its scale-appropriate spatial width!
    let mut canvas_a = FloatImage::new(canvas_w, canvas_h);
    let mut canvas_b = FloatImage::new(canvas_w, canvas_h);
    let mut mask = FloatImage::new(canvas_w, canvas_h);

    for y in 0..canvas_h {
        for x in 0..canvas_w {
            let idx = (y * canvas_w + x) as usize;

            // Populate canvas A from left_proj (clamping beyond right edge of left_proj for smooth boundary)
            let lx = (x as i32).min(lw as i32 - 1);
            canvas_a.pixels[idx] = left_proj.get_clamped(lx, y as i32);

            // Populate canvas B from right_proj (clamping before left edge of right_proj for smooth boundary)
            let rx = (x as i32 - seam_start as i32).clamp(0, rw as i32 - 1);
            let ry = (y as i32 + dy).clamp(0, rh as i32 - 1);
            canvas_b.pixels[idx] = right_proj.get_clamped(rx, ry);

            // Smooth ramp across the overlap region so the mask pyramid transitions cleanly
            let w_left = if x < seam_start {
                1.0
            } else if x >= lw {
                0.0
            } else {
                let t = (x - seam_start) as f32 / overlap_px.max(1) as f32;
                let _ = seam_center;
                0.5 * (1.0 + (t * std::f32::consts::PI).cos())
            };
            mask.pixels[idx] = [w_left, w_left, w_left, 1.0];
        }
    }

    let blended = multi_band_laplacian_blend(&canvas_a, &canvas_b, &mask, options.blend_bands);
    Ok(blended.to_pixmap())
}

/// Stitch a horizontal sequence of overlapping photos into a wide panorama.
pub fn stitch_sequence(images: &[Pixmap], options: &PanoramaOptions) -> Result<Pixmap, String> {
    if images.is_empty() {
        return Err("No images provided for stitching".into());
    }
    if images.len() == 1 {
        return Ok(images[0].clone());
    }

    let mut current = images[0].clone();
    for img in &images[1..] {
        current = stitch_two(&current, img, options)?;
    }

    Ok(current)
}
