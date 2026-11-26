//! [`BezierMesh`]: the patch grid, its evaluation, fitting and splitting.

use super::*;

/// A grid of bicubic Bezier patches over the unit square of a source box.
///
/// `us`/`vs` are the patch boundaries in the box's normalised coordinates (`0..=1`, increasing,
/// first 0 and last 1). Control points are a `(3·(us.len()-1)+1) × (3·(vs.len()-1)+1)` grid in
/// row-major order, in output (document) coordinates; neighbouring patches share their edge row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BezierMesh {
    pub us: Vec<f64>,
    pub vs: Vec<f64>,
    pub points: Vec<[f64; 2]>,
}

impl BezierMesh {
    /// The identity mesh over `bounds` with `cols × rows` patches (Photoshop's default is 1 × 1,
    /// a 4 × 4 control grid).
    pub fn identity(bounds: [f64; 4], cols: usize, rows: usize) -> BezierMesh {
        let (cols, rows) = (cols.clamp(1, 64), rows.clamp(1, 64));
        let us: Vec<f64> = (0..=cols).map(|i| i as f64 / cols as f64).collect();
        let vs: Vec<f64> = (0..=rows).map(|i| i as f64 / rows as f64).collect();
        Self::fit(
            &|s, t| {
                [
                    bounds[0] + s * (bounds[2] - bounds[0]),
                    bounds[1] + t * (bounds[3] - bounds[1]),
                ]
            },
            us,
            vs,
        )
    }

    /// Fits a mesh with patch boundaries `us`/`vs` to the map `f` (normalised → output) by
    /// interpolating it at the patch's thirds. Exact for bicubic maps (affine maps included).
    pub fn fit(f: &dyn Fn(f64, f64) -> [f64; 2], us: Vec<f64>, vs: Vec<f64>) -> BezierMesh {
        let (pc, pr) = (us.len() - 1, vs.len() - 1);
        let (nx, ny) = (3 * pc + 1, 3 * pr + 1);
        let mut points = vec![[0.0; 2]; nx * ny];
        for pj in 0..pr {
            for pi in 0..pc {
                let mut s = [[[0.0; 2]; 4]; 4]; // s[row j][col i]
                for (j, row) in s.iter_mut().enumerate() {
                    for (i, v) in row.iter_mut().enumerate() {
                        let u = us[pi] + (us[pi + 1] - us[pi]) * i as f64 / 3.0;
                        let t = vs[pj] + (vs[pj + 1] - vs[pj]) * j as f64 / 3.0;
                        *v = f(u, t);
                    }
                }
                for j in 0..4 {
                    for i in 0..4 {
                        let mut acc = [0.0; 2];
                        for (a, wa) in BERN_INV[j].iter().enumerate() {
                            for (b, wb) in BERN_INV[i].iter().enumerate() {
                                let k = wa * wb;
                                acc[0] += k * s[a][b][0];
                                acc[1] += k * s[a][b][1];
                            }
                        }
                        points[(3 * pj + j) * nx + 3 * pi + i] = acc;
                    }
                }
            }
        }
        BezierMesh { us, vs, points }
    }

    /// Control points per row.
    pub fn nx(&self) -> usize {
        3 * (self.us.len() - 1) + 1
    }
    /// Control point rows.
    pub fn ny(&self) -> usize {
        3 * (self.vs.len() - 1) + 1
    }
    pub fn point(&self, i: usize, j: usize) -> [f64; 2] {
        self.points[j * self.nx() + i]
    }

    /// Checks the invariants (knots and point count); malformed data from files is rejected.
    pub fn is_valid(&self) -> bool {
        let knots_ok = |k: &[f64]| {
            k.len() >= 2
                && k[0] == 0.0
                && k[k.len() - 1] == 1.0
                && k.windows(2).all(|w| w[1] > w[0])
        };
        knots_ok(&self.us)
            && knots_ok(&self.vs)
            && self.points.len() == self.nx() * self.ny()
            && self
                .points
                .iter()
                .all(|p| p[0].is_finite() && p[1].is_finite())
    }

    pub(super) fn locate(knots: &[f64], s: f64) -> (usize, f64) {
        let n = knots.len() - 1;
        let s = s.clamp(0.0, 1.0);
        let mut i = 0;
        while i + 1 < n && s > knots[i + 1] {
            i += 1;
        }
        let w = knots[i + 1] - knots[i];
        (i, if w > 0.0 { (s - knots[i]) / w } else { 0.0 })
    }

    /// Evaluates the surface at normalised `(s, t)`.
    pub fn eval(&self, s: f64, t: f64) -> [f64; 2] {
        let (pi, a) = Self::locate(&self.us, s);
        let (pj, b) = Self::locate(&self.vs, t);
        let (ba, bb) = (bernstein(a), bernstein(b));
        let nx = self.nx();
        let mut out = [0.0; 2];
        for (j, wb) in bb.iter().enumerate() {
            for (i, wa) in ba.iter().enumerate() {
                let p = self.points[(3 * pj + j) * nx + 3 * pi + i];
                let k = wa * wb;
                out[0] += p[0] * k;
                out[1] += p[1] * k;
            }
        }
        out
    }

    /// Splits the patch column containing `s` at `s` (a vertical split line). Exact: the surface
    /// is unchanged. Returns false when `s` is on or too close to an existing boundary.
    pub fn split_u(&mut self, s: f64) -> bool {
        let (pi, a) = Self::locate(&self.us, s);
        if !(0.01..=0.99).contains(&a) {
            return false;
        }
        let (nx, ny) = (self.nx(), self.ny());
        let mut pts = Vec::with_capacity((nx + 3) * ny);
        for j in 0..ny {
            let row = &self.points[j * nx..(j + 1) * nx];
            pts.extend_from_slice(&row[..3 * pi]);
            let seg = [
                row[3 * pi],
                row[3 * pi + 1],
                row[3 * pi + 2],
                row[3 * pi + 3],
            ];
            pts.extend_from_slice(&split_cubic(seg, a));
            pts.extend_from_slice(&row[3 * pi + 4..]);
        }
        self.points = pts;
        self.us.insert(pi + 1, s.clamp(0.0, 1.0));
        true
    }

    /// Splits the patch row containing `t` at `t` (a horizontal split line). Exact.
    pub fn split_v(&mut self, t: f64) -> bool {
        self.transpose();
        let ok = self.split_u(t);
        self.transpose();
        ok
    }

    /// Removes interior vertical boundary `k` (1..us.len()-1), merging its two patch columns.
    /// Exact when the patches came from an unedited split; otherwise the outer control points are
    /// kept and the inner handles extrapolated.
    pub fn remove_split_u(&mut self, k: usize) -> bool {
        if k == 0 || k + 1 >= self.us.len() {
            return false;
        }
        let a = (self.us[k] - self.us[k - 1]) / (self.us[k + 1] - self.us[k - 1]);
        let (nx, ny) = (self.nx(), self.ny());
        let mut pts = Vec::with_capacity((nx - 3) * ny);
        let c0 = 3 * (k - 1);
        for j in 0..ny {
            let row = &self.points[j * nx..(j + 1) * nx];
            let (p0, q1, r2, p3) = (row[c0], row[c0 + 1], row[c0 + 5], row[c0 + 6]);
            // De Casteljau inverse: q1 = lerp(p0, p1, a), r2 = lerp(p2, p3, a).
            let p1 = [
                (q1[0] - (1.0 - a) * p0[0]) / a,
                (q1[1] - (1.0 - a) * p0[1]) / a,
            ];
            let p2 = [
                (r2[0] - a * p3[0]) / (1.0 - a),
                (r2[1] - a * p3[1]) / (1.0 - a),
            ];
            pts.extend_from_slice(&row[..c0]);
            pts.extend_from_slice(&[p0, p1, p2, p3]);
            pts.extend_from_slice(&row[c0 + 7..]);
        }
        self.points = pts;
        self.us.remove(k);
        true
    }

    /// Removes interior horizontal boundary `k`.
    pub fn remove_split_v(&mut self, k: usize) -> bool {
        self.transpose();
        let ok = self.remove_split_u(k);
        self.transpose();
        ok
    }

    fn transpose(&mut self) {
        let (nx, ny) = (self.nx(), self.ny());
        let mut pts = vec![[0.0; 2]; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                pts[i * ny + j] = self.points[j * nx + i];
            }
        }
        self.points = pts;
        std::mem::swap(&mut self.us, &mut self.vs);
    }

    /// Applies `f` to every control point (exact for affine maps).
    pub fn map_points(&self, f: impl Fn([f64; 2]) -> [f64; 2]) -> BezierMesh {
        BezierMesh {
            us: self.us.clone(),
            vs: self.vs.clone(),
            points: self.points.iter().map(|p| f(*p)).collect(),
        }
    }

    /// Normalised `(s, t)` whose surface point is nearest `p` (coarse search, then Newton
    /// steps on the patch). Used to place split lines where the user clicks.
    pub fn param_at(&self, p: [f64; 2]) -> (f64, f64) {
        let n = 48;
        let mut best = (0.5, 0.5, f64::MAX);
        for j in 0..=n {
            for i in 0..=n {
                let (s, t) = (i as f64 / n as f64, j as f64 / n as f64);
                let q = self.eval(s, t);
                let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
                if d < best.2 {
                    best = (s, t, d);
                }
            }
        }
        let (mut s, mut t) = (best.0, best.1);
        for _ in 0..12 {
            let h = 1e-5;
            let q = self.eval(s, t);
            let (qs, qt) = (self.eval(s + h, t), self.eval(s, t + h));
            let (a, b, c, d) = (
                (qs[0] - q[0]) / h,
                (qt[0] - q[0]) / h,
                (qs[1] - q[1]) / h,
                (qt[1] - q[1]) / h,
            );
            let det = a * d - b * c;
            if det.abs() < 1e-12 {
                break;
            }
            let (ex, ey) = (p[0] - q[0], p[1] - q[1]);
            s = (s + (d * ex - b * ey) / det).clamp(0.0, 1.0);
            t = (t + (a * ey - c * ex) / det).clamp(0.0, 1.0);
        }
        (s, t)
    }

    /// Bounding box of the control points (the surface lies inside it).
    pub fn control_bounds(&self) -> [f64; 4] {
        self.points
            .iter()
            .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            })
    }
}
