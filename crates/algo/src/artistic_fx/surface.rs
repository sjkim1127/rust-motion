//! Noise, brush strokes, lighting and texture overlays.

use super::*;
use crate::fxutil::cell_point;
use crate::noise::hash01;
use std::f32::consts::PI;

/// Smooth value noise (0–1) on a unit lattice.
pub(super) fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (x - ix as f32, y - iy as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let h = |a: i32, b: i32| hash01(a, b, 0, seed);
    let top = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * sx;
    let bot = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * sx;
    top + (bot - top) * sy
}

/// Three-octave value noise (0–1).
pub(super) fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    (vnoise(x, y, seed) * 0.57
        + vnoise(x * 2.03, y * 2.03, seed ^ 0x51) * 0.29
        + vnoise(x * 4.1, y * 4.1, seed ^ 0xa3) * 0.14)
        / 1.0
}

/// Stroke texture: value noise stretched `len` px along the direction `d` and `wid` px across.
pub(super) fn strokes(x: i32, y: i32, d: (i32, i32), len: f32, wid: f32, seed: u32) -> f32 {
    let (dx, dy) = (d.0 as f32, d.1 as f32);
    let l = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (ux, uy) = (dx / l, dy / l);
    let (x, y) = (x as f32, y as f32);
    let u = x * ux + y * uy;
    let v = -x * uy + y * ux;
    vnoise(u / len.max(0.5), v / wid.max(0.3), seed)
}

/// Repaints in discrete strokes `len` px long and `wid` px wide along `d`: every stroke takes the
/// colour of `src` at its middle, and neighbouring strokes start at staggered points, so the
/// image reads as separate strokes rather than a smear. Also returns where the pixel lies along
/// its stroke (0 at the start, 1 at the end). Reads up to `len / 2 + 1` px away.
pub(super) fn paint_stroke(
    src: &Win,
    x: i32,
    y: i32,
    d: (i32, i32),
    len: f32,
    wid: f32,
    seed: u32,
) -> (Px, f32) {
    let (dx, dy) = (d.0 as f32, d.1 as f32);
    let l = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (ux, uy) = (dx / l, dy / l);
    let (len, wid) = (len.max(1.0), wid.max(0.5));
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let u = fx * ux + fy * uy;
    let v = -fx * uy + fy * ux;
    let lane = (v / wid).floor() as i32;
    let off = hash01(lane, 0, 0, seed) * len;
    let t = ((u + off) / len).rem_euclid(1.0);
    // Back to the stroke's middle along the direction.
    let du = (0.5 - t) * len;
    (src.sample(fx + du * ux, fy + du * uy), t)
}

/// Unit vector toward the light in image coordinates (y down).
pub(super) fn light_dir(name: &str) -> (f32, f32) {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    match name {
        "bottom" => (0.0, 1.0),
        "bottomLeft" => (-s, s),
        "left" => (-1.0, 0.0),
        "topLeft" => (-s, -s),
        "topRight" => (s, -s),
        "right" => (1.0, 0.0),
        "bottomRight" => (s, s),
        _ => (0.0, -1.0),
    }
}

/// Lambert shading factor (1 = flat) of a height field with gradient `(gx, gy)`.
#[inline]
pub(super) fn shade(gx: f32, gy: f32, light: (f32, f32), k: f32) -> f32 {
    let (nx, ny, nz) = (-k * gx, -k * gy, 1.0);
    let nl = (nx * nx + ny * ny + nz * nz).sqrt();
    let (lx, ly, lz) = (light.0, light.1, 1.0);
    let ll = (lx * lx + ly * ly + lz * lz).sqrt();
    let d = (nx * lx + ny * ly + nz * lz) / (nl * ll);
    d / (lz / ll)
}

/// Procedural textures (0–1 heights) at document coordinates; `s` scales them (1 = 100 %).
pub(super) fn texture(kind: &str, x: f32, y: f32, s: f32) -> f32 {
    let (x, y) = (x / s, y / s);
    match kind {
        "brick" => {
            let (bw, bh) = (24.0, 12.0);
            let row = (y / bh).floor();
            let xo = x + if row as i32 % 2 == 0 { 0.0 } else { bw / 2.0 };
            let (u, v) = (xo.rem_euclid(bw), y.rem_euclid(bh));
            let d = u.min(bw - u).min(v.min(bh - v));
            smoothstep(0.6, 2.2, d) * (0.85 + 0.15 * vnoise(x / 3.0, y / 3.0, 91))
        }
        "burlap" => weave(x, y, 7.0, 0.35),
        "canvas" => weave(x, y, 3.5, 0.18),
        "sandstone" => fbm(x / 7.0, y / 7.0, 93) * 0.7 + hash01(x as i32, y as i32, 0, 94) * 0.3,
        "blocks" => {
            let b = 18.0;
            let (u, v) = (x.rem_euclid(b) / b, y.rem_euclid(b) / b);
            (u.min(1.0 - u).min(v.min(1.0 - v)) * 4.0).min(1.0)
        }
        "frosted" => fbm(x / 2.5, y / 2.5, 95),
        "tinyLens" => {
            let p = 12.0;
            let (u, v) = (
                (x.rem_euclid(p) / p - 0.5) * 2.0,
                (y.rem_euclid(p) / p - 0.5) * 2.0,
            );
            (1.0 - (u * u + v * v)).max(0.0).sqrt()
        }
        _ => 0.5,
    }
}

/// Basket weave of threads `p` px wide with `noise` irregularity.
fn weave(x: f32, y: f32, p: f32, noise: f32) -> f32 {
    let (cx, cy) = ((x / p).floor() as i32, (y / p).floor() as i32);
    let over_h = (cx + cy).rem_euclid(2) == 0;
    let (u, v) = (x.rem_euclid(p) / p, y.rem_euclid(p) / p);
    // A thread is a rounded ridge across its cell.
    let ridge = if over_h {
        (v * PI).sin()
    } else {
        (u * PI).sin()
    };
    let n = vnoise(x / (p * 0.7), y / (p * 2.0), 97);
    ridge * (1.0 - noise) + n * noise
}

/// Texturizer core: relief-shades the window with a procedural texture.
pub(super) fn texturize(
    win: &mut Win,
    kind: &str,
    scaling: f32,
    relief: f32,
    light: &str,
    invert: bool,
) {
    texturize_by(win, kind, scaling, relief, light, invert, |_| 1.0);
}

/// [`texturize`] with the relief scaled per pixel by `weight` of its colour (0–1).
pub(super) fn texturize_by(
    win: &mut Win,
    kind: &str,
    scaling: f32,
    relief: f32,
    light: &str,
    invert: bool,
    weight: impl Fn(&Px) -> f32,
) {
    if relief <= 0.0 {
        return;
    }
    let s = (scaling / 100.0).clamp(0.5, 2.0);
    let l = light_dir(light);
    let k = relief / 50.0 * 3.0;
    let sign = if invert { -1.0 } else { 1.0 };
    for i in 0..win.c.len() {
        let (x, y) = win.xy(i);
        let (fx, fy) = (x as f32, y as f32);
        let gx = (texture(kind, fx + 1.0, fy, s) - texture(kind, fx - 1.0, fy, s)) * 0.5 * sign;
        let gy = (texture(kind, fx, fy + 1.0, s) - texture(kind, fx, fy - 1.0, s)) * 0.5 * sign;
        let f = 1.0 + (shade(gx, gy, l, k).clamp(0.0, 2.0) - 1.0) * weight(&win.c[i]);
        win.c[i] = map(win.c[i], |v| v * f);
    }
}

/// Closest and second-closest jittered cell points (distances and the closest point).
pub(super) fn voronoi(x: f32, y: f32, cell: f32, seed: u32) -> (f32, f32, (f32, f32)) {
    let (cx, cy) = ((x / cell).floor() as i32, (y / cell).floor() as i32);
    let (mut d1, mut d2, mut p1) = (f32::MAX, f32::MAX, (x, y));
    for j in -1..=1 {
        for i in -1..=1 {
            let p = cell_point(cx + i, cy + j, cell, seed, (0.0, 0.0));
            let d = ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                p1 = p;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, p1)
}

/// Brightens highlights (Film Grain / Smudge Stick "Highlight Area" and "Intensity").
pub(super) fn highlights(p: Px, l: f32, area: f32, intensity: f32) -> Px {
    if area <= 0.0 {
        return p;
    }
    let m = smoothstep(1.0 - area / 20.0 * 0.75, 1.02, l) * (1.0 - intensity / 10.0 * 0.6);
    map(p, |v| v + (1.0 - v) * m)
}
