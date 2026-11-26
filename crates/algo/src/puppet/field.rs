//! The coverage field (signed distance) the mesh outline is cut from.

use super::*;

/// Coverage threshold: alpha above this counts as part of the puppet.
const ALPHA_MIN: f32 = 0.05;

/// The outline field's longest side, in blocks: how finely the mesh edge follows the outline.
const FIELD_BLOCKS: f64 = 1024.0;

/// The puppet's region as a signed distance field over a block grid: negative inside the
/// opaque area grown (or shrunk) by the expansion, positive outside, in px.
pub(super) struct Field {
    pub(super) x0: f64,
    pub(super) y0: f64,
    /// Block size (px).
    pub(super) g: f64,
    pub(super) bw: usize,
    pub(super) bh: usize,
    /// Signed distance from each block centre to the opaque outline (px, negative inside).
    sd: Vec<f32>,
    expansion: f64,
}

impl Field {
    pub(super) fn new(src: &Surface, bounds: Rect, expansion: f64) -> Field {
        let long = f64::from(bounds.width().max(bounds.height()));
        let grow = expansion.max(0.0).ceil();
        let g = ((long + 2.0 * grow + 8.0) / FIELD_BLOCKS).ceil().max(1.0) as i32;
        let area = bounds.inflate(grow as i32 + 2 * g);
        let bw = ((area.width() as i32 + g - 1) / g).max(1) as usize;
        let bh = ((area.height() as i32 + g - 1) / g).max(1) as usize;
        let mut cov = vec![false; bw * bh];
        let fmt = src.format();
        let n = fmt.channels();
        let block = |bx: usize, by: usize| {
            let (x, y) = (area.x0 + bx as i32 * g, area.y0 + by as i32 * g);
            Rect::new(x, y, x + g, y + g)
        };
        if !fmt.alpha {
            for (i, c) in cov.iter_mut().enumerate() {
                *c = !block(i % bw, i / bw).intersect(&bounds).is_empty();
            }
        } else {
            // One block row per task (rows are independent).
            let scan = |by: usize, out: &mut [bool]| {
                let r = Rect::new(
                    area.x0,
                    area.y0 + by as i32 * g,
                    area.x0 + bw as i32 * g,
                    area.y0 + (by as i32 + 1) * g,
                )
                .intersect(&bounds);
                if r.is_empty() || !src.has_tiles_in(r) {
                    return;
                }
                let row = src.read_region(r);
                let rw = r.width() as usize;
                for (i, p) in row.chunks_exact(n).enumerate() {
                    if p.last().is_some_and(|&a| a > ALPHA_MIN) {
                        let x = r.x0 + (i % rw) as i32;
                        if let Some(o) = out.get_mut(((x - area.x0) / g) as usize) {
                            *o = true;
                        }
                    }
                }
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                use rayon::prelude::*;
                cov.par_chunks_mut(bw)
                    .enumerate()
                    .for_each(|(by, out)| scan(by, out));
            }
            #[cfg(target_arch = "wasm32")]
            cov.chunks_mut(bw)
                .enumerate()
                .for_each(|(by, out)| scan(by, out));
        }
        let outside = edt(&cov, true, bw, bh);
        let inside = edt(&cov, false, bw, bh);
        let gf = f64::from(g);
        let sd = cov
            .iter()
            .zip(outside.iter().zip(&inside))
            .map(|(&c, (&o, &i))| {
                if c {
                    (-(i.sqrt() - 0.5) * gf) as f32
                } else {
                    ((o.sqrt() - 0.5) * gf) as f32
                }
            })
            .collect();
        Field {
            x0: f64::from(area.x0),
            y0: f64::from(area.y0),
            g: gf,
            bw,
            bh,
            sd,
            expansion,
        }
    }

    /// The field at a block centre (exact, no interpolation).
    fn at(&self, bx: usize, by: usize) -> f64 {
        self.sd
            .get(by * self.bw + bx)
            .map_or(f64::MAX, |&d| f64::from(d) - self.expansion)
    }

    /// The field at `p` (bilinear between block centres; clamped to the grid, which ends
    /// outside the region).
    pub(super) fn value(&self, p: [f64; 2]) -> f64 {
        let fx = ((p[0] - self.x0) / self.g - 0.5).clamp(0.0, (self.bw - 1) as f64);
        let fy = ((p[1] - self.y0) / self.g - 0.5).clamp(0.0, (self.bh - 1) as f64);
        let (ix, iy) = (fx.floor() as usize, fy.floor() as usize);
        let (ix1, iy1) = ((ix + 1).min(self.bw - 1), (iy + 1).min(self.bh - 1));
        let (tx, ty) = (fx - ix as f64, fy - iy as f64);
        let top = self.at(ix, iy) * (1.0 - tx) + self.at(ix1, iy) * tx;
        let bottom = self.at(ix, iy1) * (1.0 - tx) + self.at(ix1, iy1) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    /// The outline (where the field crosses zero) as closed loops, traced by marching squares
    /// over the block centres. The field's border blocks lie outside, so every loop closes.
    pub(super) fn outline(&self) -> Vec<Vec<[f64; 2]>> {
        let (bw, bh) = (self.bw, self.bh);
        if bw < 2 || bh < 2 {
            return Vec::new();
        }
        let inside = |x: usize, y: usize| self.at(x, y) < 0.0;
        // A crossing is named by its block edge: 2·block for the edge to the right neighbour,
        // 2·block + 1 for the edge to the one below.
        let id = |x: usize, y: usize, down: bool| 2 * (y * bw + x) + usize::from(down);
        let point = |id: usize| {
            let b = id / 2;
            let (x, y) = (b % bw, b / bw);
            let (x2, y2) = if id.is_multiple_of(2) {
                (x + 1, y)
            } else {
                (x, y + 1)
            };
            let (fa, fb) = (self.at(x, y), self.at(x2, y2));
            let t = if (fa - fb).abs() > 1e-12 {
                (fa / (fa - fb)).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let (dx, dy) = ((x2 - x) as f64, (y2 - y) as f64);
            [
                self.x0 + (x as f64 + 0.5 + dx * t) * self.g,
                self.y0 + (y as f64 + 0.5 + dy * t) * self.g,
            ]
        };
        let mut links: HashMap<usize, [usize; 2]> = HashMap::new();
        let mut link = |a: usize, b: usize| {
            for (p, q) in [(a, b), (b, a)] {
                let e = links.entry(p).or_insert([usize::MAX; 2]);
                if e[0] == usize::MAX {
                    e[0] = q;
                } else {
                    e[1] = q;
                }
            }
        };
        for y in 0..bh - 1 {
            for x in 0..bw - 1 {
                let c = [
                    inside(x, y),
                    inside(x + 1, y),
                    inside(x + 1, y + 1),
                    inside(x, y + 1),
                ];
                if c.iter().all(|&v| v == c[0]) {
                    continue;
                }
                // Top, right, bottom, left.
                let edges = [
                    id(x, y, false),
                    id(x + 1, y, true),
                    id(x, y + 1, false),
                    id(x, y, true),
                ];
                let cross = [c[0] != c[1], c[1] != c[2], c[3] != c[2], c[0] != c[3]];
                let ks: Vec<usize> = (0..4).filter(|&k| cross[k]).collect();
                if let [a, b] = ks[..] {
                    link(edges[a], edges[b]);
                } else if ks.len() == 4 {
                    // A saddle: the centre decides which diagonal pair is joined.
                    let centre = self.at(x, y)
                        + self.at(x + 1, y)
                        + self.at(x + 1, y + 1)
                        + self.at(x, y + 1)
                        < 0.0;
                    if c[0] == centre {
                        link(edges[0], edges[1]);
                        link(edges[2], edges[3]);
                    } else {
                        link(edges[3], edges[0]);
                        link(edges[1], edges[2]);
                    }
                }
            }
        }
        let mut keys: Vec<usize> = links.keys().copied().collect();
        keys.sort_unstable();
        let mut seen: HashSet<usize> = HashSet::with_capacity(keys.len());
        let mut loops = Vec::new();
        for &start in &keys {
            if seen.contains(&start) {
                continue;
            }
            let mut lp = Vec::new();
            let (mut prev, mut cur) = (usize::MAX, start);
            loop {
                seen.insert(cur);
                lp.push(point(cur));
                let Some(&[a, b]) = links.get(&cur) else {
                    break;
                };
                let next = if a != prev { a } else { b };
                if next == usize::MAX || seen.contains(&next) {
                    break;
                }
                (prev, cur) = (cur, next);
            }
            if lp.len() >= 3 {
                loops.push(lp);
            }
        }
        loops
    }
}

/// Squared Euclidean distance (in blocks) from every block to the nearest block whose `mask`
/// equals `site` (Felzenszwalb & Huttenlocher's separable lower-envelope transform). About
/// 1e20 when there is none.
fn edt(mask: &[bool], site: bool, w: usize, h: usize) -> Vec<f64> {
    const FAR: f64 = 1e20;
    let mut d: Vec<f64> = mask
        .iter()
        .map(|&m| if m == site { 0.0 } else { FAR })
        .collect();
    let mut f = vec![0.0; w.max(h)];
    let mut out = vec![0.0; w.max(h)];
    let mut v = vec![0usize; w.max(h)];
    let mut z = vec![0.0; w.max(h) + 1];
    for x in 0..w {
        for (y, fy) in f[..h].iter_mut().enumerate() {
            *fy = d[y * w + x];
        }
        edt_1d(&f[..h], &mut out[..h], &mut v, &mut z);
        for (y, o) in out[..h].iter().enumerate() {
            d[y * w + x] = *o;
        }
    }
    for row in d.chunks_mut(w) {
        f[..w].copy_from_slice(row);
        edt_1d(&f[..w], row, &mut v, &mut z);
    }
    d
}

/// One-dimensional squared distance transform of the sampled function `f`.
fn edt_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let meet = |q: usize, p: usize| {
        let (qf, pf) = (q as f64, p as f64);
        ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * (qf - pf))
    };
    for q in 1..n {
        let mut s = meet(q, v[k]);
        while k > 0 && s <= z[k] {
            k -= 1;
            s = meet(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        let qf = q as f64;
        while z[k + 1] < qf {
            k += 1;
        }
        let p = v[k];
        *o = (qf - p as f64).powi(2) + f[p];
    }
}

/// Distance from `p` to the segment `a`–`b`.
pub(super) fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist2(p, [a[0] + dx * t, a[1] + dy * t]).sqrt()
}
