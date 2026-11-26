//! Outline clean-up, Delaunay triangulation and point-in-region tests.

use super::*;

/// Simplifies a closed loop (Douglas–Peucker, within `tol` px), which keeps its corners.
pub(super) fn simplify(lp: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    let n = lp.len();
    if n < 4 {
        return lp.to_vec();
    }
    let far = (1..n)
        .max_by(|&a, &b| dist2(lp[0], lp[a]).total_cmp(&dist2(lp[0], lp[b])))
        .unwrap_or(n / 2);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    // Spans (a, b) of the loop; b == n stands for point 0.
    let mut stack = vec![(0, far), (far, n)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (lp[a], lp[b % n]);
        let mut best = (0.0, 0);
        for (i, &p) in lp.iter().enumerate().take(b).skip(a + 1) {
            let d = seg_dist(p, pa, pb);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    let out: Vec<[f64; 2]> = (0..n).filter(|&i| keep[i]).map(|i| lp[i]).collect();
    if out.len() >= 3 {
        return out;
    }
    // A speck thinner than the tolerance: keep a few of its points.
    (0..n).step_by((n / 4).max(1)).map(|i| lp[i]).collect()
}

/// Splits the sides of a closed loop longer than `step` into equal parts.
pub(super) fn densify(lp: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::with_capacity(lp.len());
    for (k, &a) in lp.iter().enumerate() {
        let b = lp[(k + 1) % lp.len()];
        let parts = (dist2(a, b).sqrt() / step).ceil().clamp(1.0, 1e4) as usize;
        for i in 0..parts {
            out.push(lerp(a, b, i as f64 / parts as f64));
        }
    }
    out
}

pub(super) fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

pub(super) fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Positive when `d` lies inside the circumcircle of the positively oriented `a`, `b`, `c`.
fn in_circle(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    let (ax, ay) = (a[0] - d[0], a[1] - d[1]);
    let (bx, by) = (b[0] - d[0], b[1] - d[1]);
    let (cx, cy) = (c[0] - d[0], c[1] - d[1]);
    (ax * ax + ay * ay) * (bx * cy - cx * by) - (bx * bx + by * by) * (ax * cy - cx * ay)
        + (cx * cx + cy * cy) * (ax * by - bx * ay)
}

const NONE: usize = usize::MAX;

/// A triangle of the Delaunay triangulation: vertices (positively oriented) and the neighbour
/// across the edge opposite each vertex.
#[derive(Clone, Copy)]
pub(super) struct DTri {
    pub(super) v: [usize; 3],
    n: [usize; 3],
    pub(super) alive: bool,
}

/// Incremental Delaunay triangulation (Bowyer–Watson), started from a triangle around the area.
/// Vertices 0..3 are that triangle's.
pub(super) struct Delaunay {
    pub(super) pts: Vec<[f64; 2]>,
    pub(super) tris: Vec<DTri>,
    stamp: Vec<u32>,
    pass: u32,
    last: usize,
}

impl Delaunay {
    pub(super) fn new(lo: [f64; 2], hi: [f64; 2]) -> Delaunay {
        let (cx, cy) = (0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]));
        let d = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0) * 4.0;
        let mut pts = vec![
            [cx - 2.0 * d, cy - d],
            [cx + 2.0 * d, cy - d],
            [cx, cy + 2.0 * d],
        ];
        if orient(pts[0], pts[1], pts[2]) < 0.0 {
            pts.swap(1, 2);
        }
        Delaunay {
            pts,
            tris: vec![DTri {
                v: [0, 1, 2],
                n: [NONE; 3],
                alive: true,
            }],
            stamp: vec![0],
            pass: 0,
            last: 0,
        }
    }

    /// The triangle holding `p`: a visibility walk from the last one made.
    fn locate(&self, p: [f64; 2]) -> Option<usize> {
        let mut t = if self.tris.get(self.last).is_some_and(|t| t.alive) {
            self.last
        } else {
            self.tris.iter().position(|t| t.alive)?
        };
        for _ in 0..self.tris.len() + 3 {
            let tri = self.tris.get(t)?;
            let step = (0..3).find(|&k| {
                orient(
                    self.pts[tri.v[(k + 1) % 3]],
                    self.pts[tri.v[(k + 2) % 3]],
                    p,
                ) < 0.0
            });
            match step {
                None => return Some(t),
                Some(k) if tri.n[k] != NONE => t = tri.n[k],
                Some(_) => return None,
            }
        }
        self.tris.iter().position(|tri| {
            tri.alive
                && (0..3).all(|k| {
                    orient(
                        self.pts[tri.v[(k + 1) % 3]],
                        self.pts[tri.v[(k + 2) % 3]],
                        p,
                    ) >= 0.0
                })
        })
    }

    /// Inserts `p`; returns its vertex index (an existing vertex's when `p` coincides with it).
    pub(super) fn insert(&mut self, p: [f64; 2]) -> Option<usize> {
        let t0 = self.locate(p)?;
        if let Some(&v) = self.tris[t0]
            .v
            .iter()
            .find(|&&v| dist2(self.pts[v], p) < 1e-12)
        {
            return Some(v);
        }
        self.pass = self.pass.wrapping_add(1);
        let pass = self.pass;
        // The cavity: every triangle whose circumcircle holds `p`, grown from `t0`.
        let mut bad = vec![t0];
        self.stamp[t0] = pass;
        let mut i = 0;
        while let Some(&t) = bad.get(i) {
            i += 1;
            for k in 0..3 {
                let nb = self.tris[t].n[k];
                if nb == NONE || self.stamp[nb] == pass {
                    continue;
                }
                let [a, b, c] = self.tris[nb].v.map(|v| self.pts[v]);
                if in_circle(a, b, c, p) > 0.0 {
                    self.stamp[nb] = pass;
                    bad.push(nb);
                }
            }
        }
        let mut rim: Vec<(usize, usize, usize)> = Vec::new();
        for &t in &bad {
            let tri = self.tris[t];
            for k in 0..3 {
                let nb = tri.n[k];
                if nb == NONE || self.stamp[nb] != pass {
                    rim.push((tri.v[(k + 1) % 3], tri.v[(k + 2) % 3], nb));
                }
            }
            self.tris[t].alive = false;
        }
        let pi = self.pts.len();
        self.pts.push(p);
        let base = self.tris.len();
        for (i, &(a, b, o)) in rim.iter().enumerate() {
            self.tris.push(DTri {
                v: [a, b, pi],
                n: [NONE, NONE, o],
                alive: true,
            });
            self.stamp.push(0);
            if let Some(ot) = self.tris.get_mut(o) {
                for k in 0..3 {
                    let (x, y) = (ot.v[(k + 1) % 3], ot.v[(k + 2) % 3]);
                    if (x == b && y == a) || (x == a && y == b) {
                        ot.n[k] = base + i;
                    }
                }
            }
        }
        for (i, &(a, b, _)) in rim.iter().enumerate() {
            let across_b = rim.iter().position(|r| r.0 == b).map_or(NONE, |j| base + j);
            let across_a = rim.iter().position(|r| r.1 == a).map_or(NONE, |j| base + j);
            self.tris[base + i].n[0] = across_b;
            self.tris[base + i].n[1] = across_a;
        }
        self.last = base;
        Some(pi)
    }

    pub(super) fn edges(&self) -> HashSet<(usize, usize)> {
        let mut e = HashSet::new();
        for t in self.tris.iter().filter(|t| t.alive) {
            for k in 0..3 {
                let (a, b) = (t.v[k], t.v[(k + 1) % 3]);
                e.insert((a.min(b), a.max(b)));
            }
        }
        e
    }
}

/// Even-odd point-in-polygon test against closed outline segments, bucketed by row.
pub(super) struct Inside<'a> {
    pts: &'a [[f64; 2]],
    segs: &'a [(usize, usize)],
    y0: f64,
    row: f64,
    rows: Vec<Vec<usize>>,
}

impl<'a> Inside<'a> {
    pub(super) fn new(pts: &'a [[f64; 2]], segs: &'a [(usize, usize)], row: f64) -> Self {
        let y0 = segs
            .iter()
            .flat_map(|&(a, b)| [pts[a][1], pts[b][1]])
            .fold(f64::MAX, f64::min);
        let y1 = segs
            .iter()
            .flat_map(|&(a, b)| [pts[a][1], pts[b][1]])
            .fold(f64::MIN, f64::max);
        let n = if y1 >= y0 {
            ((y1 - y0) / row).floor().clamp(0.0, 1e5) as usize + 1
        } else {
            0
        };
        let mut rows = vec![Vec::new(); n];
        for (i, &(a, b)) in segs.iter().enumerate() {
            let (lo, hi) = (pts[a][1].min(pts[b][1]), pts[a][1].max(pts[b][1]));
            let (r0, r1) = (
                ((lo - y0) / row).floor() as usize,
                ((hi - y0) / row).floor() as usize,
            );
            for r in rows.iter_mut().take(r1 + 1).skip(r0) {
                r.push(i);
            }
        }
        Inside {
            pts,
            segs,
            y0,
            row,
            rows,
        }
    }

    pub(super) fn contains(&self, p: [f64; 2]) -> bool {
        if p[1] < self.y0 {
            return false;
        }
        let Some(row) = self
            .rows
            .get(((p[1] - self.y0) / self.row).floor() as usize)
        else {
            return false;
        };
        let mut odd = false;
        for &i in row {
            let (a, b) = (self.pts[self.segs[i].0], self.pts[self.segs[i].1]);
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
            {
                odd = !odd;
            }
        }
        odd
    }
}
