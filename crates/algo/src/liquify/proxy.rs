//! The downsampled proxy image behind the interactive preview.

use super::*;

/// A downsampled, premultiplied RGBA copy of a layer for the interactive preview.
#[derive(Clone, Debug)]
pub struct ProxyImage {
    /// Document region the proxy covers (the field bounds).
    pub bounds: Rect,
    /// Document pixels per proxy pixel.
    pub scale: f64,
    pub w: usize,
    pub h: usize,
    /// Premultiplied RGBA, row-major.
    pub px: Vec<[f32; 4]>,
}

const MAX_FULL_RES_SOURCE_PIXELS: usize = 8 * 1024 * 1024;

impl ProxyImage {
    /// Box-downsamples `src` over `bounds` so the longer side is at most `max_side`.
    pub fn new(src: &Surface, bounds: Rect, max_side: usize) -> Self {
        let side = bounds.width().max(bounds.height()).max(1) as f64;
        let scale = (side / max_side.max(1) as f64).max(1.0).ceil();
        let w = ((f64::from(bounds.width()) / scale).ceil() as usize).max(1);
        let h = ((f64::from(bounds.height()) / scale).ceil() as usize).max(1);
        let k = scale as i32;
        let mut px = vec![[0.0f32; 4]; w * h];
        let fmt = src.format();
        let fill_rows = |y: usize, row: &mut [[f32; 4]]| {
            let r = Rect::new(
                bounds.x0,
                bounds.y0 + y as i32 * k,
                bounds.x0 + w as i32 * k,
                (bounds.y0 + (y as i32 + 1) * k).min(bounds.y1),
            );
            let mut buf = vec![[0.0f32; 4]; r.width() as usize * r.height() as usize];
            src.read_rgba_into(r, &mut buf);
            let rw = r.width() as usize;
            for (x, o) in row.iter_mut().enumerate() {
                let mut acc = [0.0f32; 4];
                let mut cnt = 0.0f32;
                for yy in 0..r.height() as usize {
                    for xx in x * k as usize..((x + 1) * k as usize).min(rw) {
                        let p = buf[yy * rw + xx];
                        let a = if fmt.alpha { p[3] } else { 1.0 };
                        acc[0] += p[0] * a;
                        acc[1] += p[1] * a;
                        acc[2] += p[2] * a;
                        acc[3] += a;
                        cnt += 1.0;
                    }
                }
                if cnt > 0.0 {
                    *o = acc.map(|v| v / cnt);
                }
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            px.par_chunks_mut(w)
                .enumerate()
                .for_each(|(y, row)| fill_rows(y, row));
        }
        #[cfg(target_arch = "wasm32")]
        px.chunks_mut(w)
            .enumerate()
            .for_each(|(y, row)| fill_rows(y, row));
        ProxyImage {
            bounds,
            scale,
            w,
            h,
            px,
        }
    }

    /// Renders proxy pixels `r` (proxy coordinates) through the field into `out` (premultiplied
    /// RGBA8, `w × h`): bilinear field, bilinear source.
    pub fn render(&self, field: &LiquifyField, r: [usize; 4], out: &mut [[u8; 4]]) {
        let [x0, y0, x1, y1] = [
            r[0].min(self.w),
            r[1].min(self.h),
            r[2].min(self.w),
            r[3].min(self.h),
        ];
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let s = self.scale;
        let (bx, by) = (f64::from(self.bounds.x0), f64::from(self.bounds.y0));
        let row = |y: usize, line: &mut [[u8; 4]]| {
            let py = by + (y as f64 + 0.5) * s;
            for (x, slot) in line.iter_mut().enumerate().take(x1).skip(x0) {
                let px = bx + (x as f64 + 0.5) * s;
                let d = field.sample(px, py);
                let (u, v) = ((px + d[0] - bx) / s - 0.5, (py + d[1] - by) / s - 0.5);
                let (iu, iv) = (u.floor(), v.floor());
                let (fu, fv) = ((u - iu) as f32, (v - iv) as f32);
                let at = |x: f64, y: f64| {
                    self.px[(y.clamp(0.0, (self.h - 1) as f64) as usize) * self.w
                        + x.clamp(0.0, (self.w - 1) as f64) as usize]
                };
                let (a, b, c, e) = (
                    at(iu, iv),
                    at(iu + 1.0, iv),
                    at(iu, iv + 1.0),
                    at(iu + 1.0, iv + 1.0),
                );
                *slot = std::array::from_fn(|k| {
                    let v = (a[k] * (1.0 - fu) + b[k] * fu) * (1.0 - fv)
                        + (c[k] * (1.0 - fu) + e[k] * fu) * fv;
                    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                });
            }
        };
        let rows = &mut out[y0 * self.w..y1 * self.w];
        // A dab's few thousand pixels are faster on this thread than waking the pool.
        #[cfg(not(target_arch = "wasm32"))]
        if (x1 - x0) * (y1 - y0) > 150_000 {
            use rayon::prelude::*;
            rows.par_chunks_mut(self.w)
                .enumerate()
                .for_each(|(i, line)| row(y0 + i, line));
            return;
        }
        rows.chunks_mut(self.w)
            .enumerate()
            .for_each(|(i, line)| row(y0 + i, line));
    }

    /// Renders a document-pixel-resolution preview for `region` into the corresponding part of
    /// `out`, whose dimensions cover `bounds`. Source reads are limited to the dirty region plus
    /// the displacement range that can affect it, so zoomed-in brush updates do not flatten the
    /// whole layer. `source_scratch` is reused by the caller between updates.
    pub fn render_full_resolution(
        src: &Surface,
        field: &LiquifyField,
        bounds: Rect,
        region: Rect,
        out: &mut [[u8; 4]],
        source_scratch: &mut Vec<[f32; 4]>,
    ) -> bool {
        let bounds = bounds.intersect(&field.bounds);
        let region = region.intersect(&bounds);
        let (width, height) = (bounds.width() as usize, bounds.height() as usize);
        let Some(out_len) = width.checked_mul(height) else {
            return false;
        };
        if region.is_empty() || out.len() != out_len || width == 0 || height == 0 {
            return false;
        }

        // Bilinear field interpolation is a convex combination of its surrounding nodes. The
        // extrema of those nodes therefore bound every source sample in this output rectangle.
        let (mut min_dx, mut min_dy, mut max_dx, mut max_dy) = (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        );
        let (gx0, gy0) = field.to_grid(f64::from(region.x0) + 0.5, f64::from(region.y0) + 0.5);
        let (gx1, gy1) = field.to_grid(f64::from(region.x1) - 0.5, f64::from(region.y1) - 0.5);
        let (i0, j0) = (gx0.floor().max(0.0) as usize, gy0.floor().max(0.0) as usize);
        let (i1, j1) = (gx1.ceil().max(0.0) as usize, gy1.ceil().max(0.0) as usize);
        for j in j0..=j1.min(field.h.saturating_sub(1)) {
            for i in i0..=i1.min(field.w.saturating_sub(1)) {
                let Some(v) = j
                    .checked_mul(field.w)
                    .and_then(|row| row.checked_add(i))
                    .and_then(|idx| field.d.get(idx))
                else {
                    continue;
                };
                min_dx = min_dx.min(v[0]);
                min_dy = min_dy.min(v[1]);
                max_dx = max_dx.max(v[0]);
                max_dy = max_dy.max(v[1]);
            }
        }
        if !min_dx.is_finite() || !min_dy.is_finite() || !max_dx.is_finite() || !max_dy.is_finite()
        {
            return false;
        }

        // Source coordinates are pixel centres. Add one sample on each side for bilinear
        // interpolation, then clamp to the document just as the regular proxy renderer does.
        let x0 = (f64::from(region.x0) + f64::from(min_dx)).floor() - 1.0;
        let y0 = (f64::from(region.y0) + f64::from(min_dy)).floor() - 1.0;
        let x1 = (f64::from(region.x1 - 1) + f64::from(max_dx)).ceil() + 2.0;
        let y1 = (f64::from(region.y1 - 1) + f64::from(max_dy)).ceil() + 2.0;
        let canvas = field.bounds;
        let src_rect = Rect::new(
            x0.max(f64::from(canvas.x0)).min(f64::from(canvas.x1)) as i32,
            y0.max(f64::from(canvas.y0)).min(f64::from(canvas.y1)) as i32,
            x1.max(f64::from(canvas.x0)).min(f64::from(canvas.x1)) as i32,
            y1.max(f64::from(canvas.y0)).min(f64::from(canvas.y1)) as i32,
        );
        if src_rect.is_empty() {
            return false;
        }
        let (sw, sh) = (src_rect.width() as usize, src_rect.height() as usize);
        let Some(source_len) = sw.checked_mul(sh) else {
            return false;
        };
        if source_len > MAX_FULL_RES_SOURCE_PIXELS {
            return false;
        }
        if source_scratch
            .try_reserve(source_len.saturating_sub(source_scratch.len()))
            .is_err()
        {
            return false;
        }
        source_scratch.resize(source_len, [0.0; 4]);
        src.read_rgba_into(src_rect, source_scratch);
        if !src.format().alpha {
            for pixel in source_scratch.iter_mut() {
                pixel[3] = 1.0;
            }
        }
        for pixel in source_scratch.iter_mut() {
            let alpha = pixel[3];
            pixel[0] *= alpha;
            pixel[1] *= alpha;
            pixel[2] *= alpha;
        }

        let x0 = (region.x0 - bounds.x0) as usize;
        let x1 = (region.x1 - bounds.x0) as usize;
        let y0 = (region.y0 - bounds.y0) as usize;
        let y1 = (region.y1 - bounds.y0) as usize;
        let source_at = |x: f64, y: f64| -> [f32; 4] {
            let fx = (x - 0.5).clamp(f64::from(canvas.x0), f64::from(canvas.x1 - 1));
            let fy = (y - 0.5).clamp(f64::from(canvas.y0), f64::from(canvas.y1 - 1));
            let ix = fx.floor();
            let iy = fy.floor();
            let (tx, ty) = ((fx - ix) as f32, (fy - iy) as f32);
            let sx0 = (ix as i32).clamp(src_rect.x0, src_rect.x1 - 1);
            let sy0 = (iy as i32).clamp(src_rect.y0, src_rect.y1 - 1);
            let sx1 = (sx0 + 1).min(src_rect.x1 - 1);
            let sy1 = (sy0 + 1).min(src_rect.y1 - 1);
            let at = |sx: i32, sy: i32| -> [f32; 4] {
                let ox = (sx - src_rect.x0) as usize;
                let oy = (sy - src_rect.y0) as usize;
                source_scratch
                    .get(oy * sw + ox)
                    .copied()
                    .unwrap_or([0.0; 4])
            };
            let (a, b, c, d) = (at(sx0, sy0), at(sx1, sy0), at(sx0, sy1), at(sx1, sy1));
            std::array::from_fn(|channel| {
                (a[channel] * (1.0 - tx) + b[channel] * tx) * (1.0 - ty)
                    + (c[channel] * (1.0 - tx) + d[channel] * tx) * ty
            })
        };
        let render_rows = |first_y: usize, rows: &mut [[u8; 4]]| {
            for (offset, row) in rows.chunks_mut(width).enumerate() {
                let y = bounds.y0 + (first_y + offset) as i32;
                let Some(dest) = row.get_mut(x0..x1) else {
                    continue;
                };
                for (offset_x, pixel) in dest.iter_mut().enumerate() {
                    let x = bounds.x0 + (x0 + offset_x) as i32;
                    let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
                    let d = field.sample(p[0], p[1]);
                    let rgba = source_at(p[0] + d[0], p[1] + d[1]);
                    *pixel = rgba.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                }
            }
        };
        let Some(rows) = out.get_mut(y0 * width..y1 * width) else {
            return false;
        };
        #[cfg(not(target_arch = "wasm32"))]
        if (x1 - x0).saturating_mul(y1 - y0) > 150_000 {
            use rayon::prelude::*;
            rows.par_chunks_mut(width)
                .enumerate()
                .for_each(|(offset, line)| render_rows(y0 + offset, line));
            return true;
        }
        rows.chunks_mut(width)
            .enumerate()
            .for_each(|(offset, line)| render_rows(y0 + offset, line));
        true
    }

    /// The proxy pixel rectangle covering document rect `r`.
    pub fn proxy_rect(&self, r: Rect) -> [usize; 4] {
        let s = self.scale;
        let f = |v: i32, o: i32| (f64::from(v - o) / s).max(0.0);
        [
            f(r.x0, self.bounds.x0).floor() as usize,
            f(r.y0, self.bounds.y0).floor() as usize,
            (f(r.x1, self.bounds.x0).ceil() as usize + 1).min(self.w),
            (f(r.y1, self.bounds.y0).ceil() as usize + 1).min(self.h),
        ]
    }
}
