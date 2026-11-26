//! Resampling the source through the field: brush falloff, interpolation kernels and the full-quality render.

use super::*;

/// A dab's strength at squared distance `t2` (in brush radii, 0..1) for Density `density` (0..1):
/// 1 at the centre, and down to 0 at the rim with a flat slope at every density, as in
/// Photoshop, where Density sets how far the full effect reaches toward the edge (0: only the
/// centre, the rest feathered; 1: most of the brush) but the effect always fades out at it.
/// A brush that stopped short of 0 at its rim moved the pixels inside it and left those just
/// outside, tearing the image along every dab's circle (#2606).
pub fn falloff(t2: f64, density: f64) -> f64 {
    let t2 = t2.clamp(0.0, 1.0);
    let density = if density.is_nan() {
        0.5
    } else {
        density.clamp(0.0, 1.0)
    };
    // Feathered: (1 - t²)³. Dense: (1 - t⁴)², flat in the middle. Both are 0 with zero slope at
    // t = 1, and their slopes stay small enough that a dab every fifth of the radius never folds
    // the image over itself.
    let feathered = (1.0 - t2).powi(3);
    let dense = (1.0 - t2 * t2).powi(2);
    feathered + (dense - feathered) * density
}

pub(super) fn bilinear(d: &[[f32; 2]], w: usize, h: usize, gx: f64, gy: f64) -> [f64; 2] {
    if !(gx > -1.0 && gy > -1.0 && gx < w as f64 && gy < h as f64) {
        return [0.0, 0.0];
    }
    let gx = gx.clamp(0.0, (w - 1) as f64);
    let gy = gy.clamp(0.0, (h - 1) as f64);
    let (i, j) = (
        (gx as usize).min(w.saturating_sub(2)),
        (gy as usize).min(h.saturating_sub(2)),
    );
    let (fx, fy) = (gx - i as f64, gy - j as f64);
    let i1 = (i + 1).min(w - 1);
    let j1 = (j + 1).min(h - 1);
    let at = |x: usize, y: usize| d[y * w + x];
    let (a, b, c, e) = (at(i, j), at(i1, j), at(i, j1), at(i1, j1));
    let l = |k: usize| {
        (f64::from(a[k]) * (1.0 - fx) + f64::from(b[k]) * fx) * (1.0 - fy)
            + (f64::from(c[k]) * (1.0 - fx) + f64::from(e[k]) * fx) * fy
    };
    [l(0), l(1)]
}

fn catmull_rom(t: f64) -> [f64; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

/// Resamples `src` through the field: inside `field.bounds` each pixel shows the source at
/// `p + d(p)` (bicubic field, bicubic premultiplied source, edge pixels repeat past the field
/// bounds); everything outside the bounds is kept. Same format as `src`.
pub fn apply_liquify(src: &Surface, field: &LiquifyField) -> Surface {
    let fmt = src.format();
    let n = fmt.channels();
    let alpha = fmt.alpha;
    let b = field.bounds;
    let mut out = src.clone();
    if field.is_identity() || b.is_empty() {
        return out;
    }
    let maxd = f64::from(field.max_displacement()).ceil() as i32 + 3;
    let tiles: Vec<Rect> = b
        .tiles()
        .map(|t| t.rect().intersect(&b))
        .filter(|r| !r.is_empty() && field.touches(*r))
        .collect();
    let work = |t: &Rect| -> Option<(Rect, Vec<f32>)> {
        let w = t.width() as usize;
        let h = t.height() as usize;
        // Field per pixel (bicubic), rows share their y weights.
        let mut disp = vec![[0.0f64; 2]; w * h];
        let (mut fx0, mut fy0, mut fx1, mut fy1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut moved = false;
        for y in 0..h {
            let py = f64::from(t.y0) + y as f64 + 0.5;
            let gy = (py - f64::from(b.y0) - 0.5) / field.cell;
            let wy = catmull_rom(gy - gy.floor());
            for x in 0..w {
                let px = f64::from(t.x0) + x as f64 + 0.5;
                let gx = (px - f64::from(b.x0) - 0.5) / field.cell;
                let wx = catmull_rom(gx - gx.floor());
                let d = field.sample_cubic(gx, gy, &wx, &wy);
                if d[0] != 0.0 || d[1] != 0.0 {
                    moved = true;
                }
                disp[y * w + x] = d;
                let (sx, sy) = (px + d[0], py + d[1]);
                fx0 = fx0.min(sx);
                fy0 = fy0.min(sy);
                fx1 = fx1.max(sx);
                fy1 = fy1.max(sy);
            }
        }
        if !moved {
            return None;
        }
        let foot = Rect::new(
            fx0.floor() as i32 - 3,
            fy0.floor() as i32 - 3,
            fx1.ceil() as i32 + 3,
            fy1.ceil() as i32 + 3,
        )
        .intersect(&t.inflate(maxd))
        .intersect(&b);
        if foot.is_empty() {
            return None;
        }
        let mut px = src.read_region(foot);
        if alpha {
            for p in px.chunks_exact_mut(n) {
                let al = p[n - 1];
                for v in &mut p[..n - 1] {
                    *v *= al;
                }
            }
        }
        let fw = foot.width() as usize;
        let at = |x: i32, y: i32| -> usize {
            let x = x.clamp(foot.x0, foot.x1 - 1);
            let y = y.clamp(foot.y0, foot.y1 - 1);
            ((y - foot.y0) as usize * fw + (x - foot.x0) as usize) * n
        };
        let snap = |t: f64| {
            if (t - t.round()).abs() < 1e-7 {
                t.round()
            } else {
                t
            }
        };
        let mut outp = vec![0.0f32; w * h * n];
        let mut acc = [0.0f64; 8];
        for y in 0..h {
            for x in 0..w {
                let d = disp[y * w + x];
                let u = snap(f64::from(t.x0) + x as f64 + d[0]);
                let v = snap(f64::from(t.y0) + y as f64 + d[1]);
                let (ix, iy) = (u.floor() as i32, v.floor() as i32);
                let (wx, wy) = (
                    catmull_rom(u - f64::from(ix)),
                    catmull_rom(v - f64::from(iy)),
                );
                acc[..n].fill(0.0);
                for (j, wyj) in wy.iter().enumerate() {
                    for (i, wxi) in wx.iter().enumerate() {
                        let k = wxi * wyj;
                        if k == 0.0 {
                            continue;
                        }
                        let o = at(ix - 1 + i as i32, iy - 1 + j as i32);
                        for (c, s) in acc.iter_mut().enumerate().take(n) {
                            *s += f64::from(px[o + c]) * k;
                        }
                    }
                }
                let o = (y * w + x) * n;
                if alpha {
                    let al = acc[n - 1].clamp(0.0, 1.0);
                    outp[o + n - 1] = al as f32;
                    for c in 0..n - 1 {
                        outp[o + c] = if al > 0.0 {
                            (acc[c] / al).clamp(0.0, 1.0) as f32
                        } else {
                            0.0
                        };
                    }
                } else {
                    for c in 0..n {
                        outp[o + c] = acc[c].clamp(0.0, 1.0) as f32;
                    }
                }
            }
        }
        Some((*t, outp))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let done: Vec<(Rect, Vec<f32>)> = {
        use rayon::prelude::*;
        tiles.par_iter().filter_map(work).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let done: Vec<(Rect, Vec<f32>)> = tiles.iter().filter_map(work).collect();
    for (r, v) in done {
        out.write_region(r, &v);
    }
    if alpha {
        out.prune();
    }
    out
}
