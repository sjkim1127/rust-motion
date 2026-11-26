//! Rasterizing a primitive list tile by tile and compositing it over a surface.

use super::*;

#[inline]
fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Rasterizes `prims` (in order) into a premultiplied RGBA buffer covering `t`.
fn raster_tile(prims: &[Prim], idx: &[u32], t: Rect) -> Vec<[f32; 4]> {
    let w = t.width() as usize;
    let mut buf = vec![[0.0f32; 4]; w * t.height() as usize];
    for &k in idx {
        let p = &prims[k as usize];
        let r = p.bounds().intersect(&t);
        if r.is_empty() {
            continue;
        }
        for y in r.y0..r.y1 {
            let row = (y - t.y0) as usize * w;
            for x in r.x0..r.x1 {
                let (cov, ct) = p.eval(x as f32 + 0.5, y as f32 + 0.5);
                if cov <= 0.0 {
                    continue;
                }
                let c = if p.c0 == p.c1 {
                    p.c0
                } else {
                    lerp4(p.c0, p.c1, ct)
                };
                let a = (c[3] * cov).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let d = &mut buf[row + (x - t.x0) as usize];
                match p.blend {
                    Blend::Add => {
                        d[0] += c[0] * a;
                        d[1] += c[1] * a;
                        d[2] += c[2] * a;
                        d[3] += a * (1.0 - d[3]);
                    }
                    Blend::Lighten => {
                        d[0] = d[0].max(c[0] * a);
                        d[1] = d[1].max(c[1] * a);
                        d[2] = d[2].max(c[2] * a);
                        d[3] = d[3].max(a);
                    }
                    Blend::Normal => {
                        let k = 1.0 - a;
                        d[0] = c[0] * a + d[0] * k;
                        d[1] = c[1] * a + d[1] * k;
                        d[2] = c[2] * a + d[2] * k;
                        d[3] = a + d[3] * k;
                    }
                }
            }
        }
    }
    buf
}

/// Size of the tiles [`composite`] works in.
const CTILE: i32 = 128;

/// Draws `prims` over `surface` inside `clip`, mixed by the selection coverage
/// (channel 0 of `selection`). Returns the bounds of what was drawn.
pub fn composite(
    surface: &mut Surface,
    prims: &[Prim],
    clip: Rect,
    selection: Option<&Surface>,
) -> Rect {
    let mut used = Rect::EMPTY;
    let bounds: Vec<Rect> = prims.iter().map(|p| p.bounds().intersect(&clip)).collect();
    for b in &bounds {
        if !b.is_empty() {
            used = if used.is_empty() { *b } else { used.union(b) };
        }
    }
    if used.is_empty() {
        return used;
    }
    // Bin primitives into tiles (order preserved within a tile).
    let tx0 = used.x0.div_euclid(CTILE);
    let ty0 = used.y0.div_euclid(CTILE);
    let tx1 = (used.x1 - 1).div_euclid(CTILE);
    let ty1 = (used.y1 - 1).div_euclid(CTILE);
    let (nx, ny) = ((tx1 - tx0 + 1) as usize, (ty1 - ty0 + 1) as usize);
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
    for (k, b) in bounds.iter().enumerate() {
        if b.is_empty() {
            continue;
        }
        for ty in b.y0.div_euclid(CTILE)..=(b.y1 - 1).div_euclid(CTILE) {
            for tx in b.x0.div_euclid(CTILE)..=(b.x1 - 1).div_euclid(CTILE) {
                bins[(ty - ty0) as usize * nx + (tx - tx0) as usize].push(k as u32);
            }
        }
    }
    let fmt = surface.format();
    let n = fmt.channels();
    let jobs: Vec<(Rect, &Vec<u32>)> = bins
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.is_empty())
        .map(|(i, b)| {
            let (tx, ty) = (tx0 + (i % nx) as i32, ty0 + (i / nx) as i32);
            (
                Rect::new(tx * CTILE, ty * CTILE, (tx + 1) * CTILE, (ty + 1) * CTILE)
                    .intersect(&clip),
                b,
            )
        })
        .filter(|(r, _)| !r.is_empty())
        .collect();
    let src: &Surface = surface;
    let run = |(t, idx): &(Rect, &Vec<u32>)| -> (Rect, Vec<f32>) {
        let paint = raster_tile(prims, idx, *t);
        let mut data = src.read_region(*t);
        let sel = selection.map(|s| s.read_region(*t));
        let sn = selection.map_or(1, Surface::channels);
        let mut tmp = [0.0f32; 8];
        for (i, px) in data.chunks_exact_mut(n).enumerate() {
            let s = paint[i];
            let k = sel.as_ref().map_or(1.0, |v| v[i * sn].clamp(0.0, 1.0));
            let sa = s[3].clamp(0.0, 1.0) * k;
            if sa <= 0.0 {
                continue;
            }
            let inv = 1.0 / s[3].max(1e-6);
            let sc = [
                (s[0] * inv).clamp(0.0, 1.0),
                (s[1] * inv).clamp(0.0, 1.0),
                (s[2] * inv).clamp(0.0, 1.0),
            ];
            let d = to_rgba(&fmt, px);
            let da = if fmt.alpha { d[3] } else { 1.0 };
            let oa = sa + da * (1.0 - sa);
            let mix = |sv: f32, dv: f32| {
                if oa > 1e-7 {
                    (sv * sa + dv * da * (1.0 - sa)) / oa
                } else {
                    0.0
                }
            };
            let o = [mix(sc[0], d[0]), mix(sc[1], d[1]), mix(sc[2], d[2]), oa];
            let m = from_rgba_into(&fmt, o, &mut tmp);
            px.copy_from_slice(&tmp[..m.min(n)]);
        }
        (*t, data)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let results: Vec<(Rect, Vec<f32>)> = {
        use rayon::prelude::*;
        jobs.par_iter().map(run).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let results: Vec<(Rect, Vec<f32>)> = jobs.iter().map(run).collect();
    for (t, data) in results {
        surface.write_region(t, &data);
    }
    used
}
