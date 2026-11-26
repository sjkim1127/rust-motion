//! The as-rigid-as-possible solver: cotangent weights, pin constraints and the local/global iteration.

use super::*;

/// A pin bound to the mesh: barycentric weights on a triangle's vertices.
#[derive(Clone, Debug)]
pub(super) struct Bound {
    pub(super) verts: [usize; 3],
    pub(super) bary: [f64; 3],
}

/// Prepared ARAP solver for one mesh and a set of pin source positions.
pub struct PuppetSolver {
    pub mesh: PuppetMesh,
    /// Cotangent weight of each triangle's edges: `w[t][k]` for the edge opposite corner `k`.
    w: Vec<[f64; 3]>,
    pub(super) bound: Vec<Bound>,
    /// Triangles whose rotation a pin fixes: (pin index, triangles).
    pin_tris: Vec<Vec<usize>>,
    /// Vertices of components without pins (held at rest).
    anchored: Vec<bool>,
    matrix: Csr,
    chol: Option<BandChol>,
    pin_weight: f64,
}

fn cot(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    // Cotangent of the angle at `a`.
    let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]);
    let cross = (u[0] * v[1] - u[1] * v[0]).abs();
    if cross < 1e-12 {
        0.0
    } else {
        (u[0] * v[0] + u[1] * v[1]) / cross
    }
}

pub(super) fn locate(mesh: &PuppetMesh, p: [f64; 2]) -> Bound {
    for t in &mesh.tris {
        let [a, b, c] = t.map(|i| mesh.verts[i]);
        let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if det.abs() < 1e-12 {
            continue;
        }
        let l1 = ((p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1])) / det;
        let l2 = ((b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1])) / det;
        let l0 = 1.0 - l1 - l2;
        if l0 >= -1e-9 && l1 >= -1e-9 && l2 >= -1e-9 {
            return Bound {
                verts: *t,
                bary: [l0, l1, l2],
            };
        }
    }
    // Outside the mesh: the nearest vertex.
    let i = (0..mesh.verts.len())
        .min_by(|&a, &b| dist2(mesh.verts[a], p).total_cmp(&dist2(mesh.verts[b], p)))
        .unwrap_or(0);
    Bound {
        verts: [i, i, i],
        bary: [1.0, 0.0, 0.0],
    }
}

pub(super) fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

impl PuppetSolver {
    /// Prepares the solver for pins placed at `pin_src` (rotation-fixed pins listed in
    /// `fixed_rotation`).
    pub fn new(mesh: PuppetMesh, pin_src: &[[f64; 2]], fixed_rotation: &[bool]) -> Self {
        let nv = mesh.verts.len();
        let w: Vec<[f64; 3]> = mesh
            .tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| mesh.verts[i]);
                [0.5 * cot(a, b, c), 0.5 * cot(b, c, a), 0.5 * cot(c, a, b)]
            })
            .collect();
        // Without vertices there is nothing to bind a pin to (`locate` would fall back to a vertex 0
        // that doesn't exist, #951); every pin loop zips `bound`, so the pins are then ignored.
        let bound: Vec<Bound> = if nv == 0 {
            Vec::new()
        } else {
            pin_src.iter().map(|p| locate(&mesh, *p)).collect()
        };
        // Connected components (union-find over triangle edges).
        let mut parent: Vec<usize> = (0..nv).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        for t in &mesh.tris {
            for k in 1..3 {
                let (a, b) = (find(&mut parent, t[0]), find(&mut parent, t[k]));
                if a != b {
                    parent[a] = b;
                }
            }
        }
        let mut pinned_root = vec![false; nv];
        for b in &bound {
            for &v in &b.verts {
                let r = find(&mut parent, v);
                pinned_root[r] = true;
            }
        }
        let anchored: Vec<bool> = (0..nv)
            .map(|v| !pinned_root[find(&mut parent, v)])
            .collect();
        // Rotation-fixed pins: the triangles around the pin's nearest vertex.
        let pin_tris: Vec<Vec<usize>> = bound
            .iter()
            .zip(pin_src)
            .enumerate()
            .map(|(k, (b, p))| {
                if !fixed_rotation.get(k).copied().unwrap_or(false) {
                    return Vec::new();
                }
                let v = *b
                    .verts
                    .iter()
                    .min_by(|&&a, &&c| {
                        dist2(mesh.verts[a], *p).total_cmp(&dist2(mesh.verts[c], *p))
                    })
                    .unwrap_or(&0);
                mesh.tris
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.contains(&v))
                    .map(|(i, _)| i)
                    .collect()
            })
            .collect();
        let mut trip: Vec<(usize, usize, f64)> = Vec::with_capacity(mesh.tris.len() * 12 + nv);
        let mut diag_sum = 0.0;
        for (t, wt) in mesh.tris.iter().zip(&w) {
            for k in 0..3 {
                let (i, j) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                let wk = wt[k];
                trip.push((i, i, wk));
                trip.push((j, j, wk));
                trip.push((i, j, -wk));
                trip.push((j, i, -wk));
                diag_sum += 2.0 * wk;
            }
        }
        let pin_weight = 1e4 * (diag_sum / nv.max(1) as f64).max(1.0);
        for b in &bound {
            for a in 0..3 {
                for c in 0..3 {
                    trip.push((b.verts[a], b.verts[c], pin_weight * b.bary[a] * b.bary[c]));
                }
            }
        }
        for (v, &an) in anchored.iter().enumerate() {
            if an {
                trip.push((v, v, pin_weight));
            }
        }
        let matrix = Csr::from_triplets(nv, trip);
        let chol = BandChol::new(&matrix);
        PuppetSolver {
            mesh,
            w,
            bound,
            pin_tris,
            anchored,
            matrix,
            chol,
            pin_weight,
        }
    }

    /// The ARAP energy of `v` for the given per-triangle linear maps (plus the pin terms).
    fn energy_with(&self, v: &[[f64; 2]], rots: &[[f64; 4]], pin_dst: &[[f64; 2]]) -> f64 {
        let x = &self.mesh.verts;
        let mut e = 0.0;
        for ((t, wt), r) in self.mesh.tris.iter().zip(&self.w).zip(rots) {
            for k in 0..3 {
                let (i, j) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                let d = [v[i][0] - v[j][0], v[i][1] - v[j][1]];
                let ex = [x[i][0] - x[j][0], x[i][1] - x[j][1]];
                let re = [r[0] * ex[0] + r[1] * ex[1], r[2] * ex[0] + r[3] * ex[1]];
                e += wt[k] * ((d[0] - re[0]).powi(2) + (d[1] - re[1]).powi(2));
            }
        }
        for (b, d) in self.bound.iter().zip(pin_dst) {
            let p = (0..3).fold([0.0, 0.0], |a, k| {
                [
                    a[0] + b.bary[k] * v[b.verts[k]][0],
                    a[1] + b.bary[k] * v[b.verts[k]][1],
                ]
            });
            e += self.pin_weight * dist2(p, *d);
        }
        e
    }

    fn local(&self, v: &[[f64; 2]], mode: PuppetMode, fixed: &[Option<f64>]) -> Vec<[f64; 4]> {
        let x = &self.mesh.verts;
        let mut rots: Vec<[f64; 4]> = self
            .mesh
            .tris
            .iter()
            .zip(&self.w)
            .map(|(t, wt)| {
                let mut s = [0.0f64; 4];
                let mut norm = 0.0;
                for k in 0..3 {
                    let (i, j) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                    let d = [v[i][0] - v[j][0], v[i][1] - v[j][1]];
                    let e = [x[i][0] - x[j][0], x[i][1] - x[j][1]];
                    s[0] += wt[k] * d[0] * e[0];
                    s[1] += wt[k] * d[0] * e[1];
                    s[2] += wt[k] * d[1] * e[0];
                    s[3] += wt[k] * d[1] * e[1];
                    norm += wt[k] * (e[0] * e[0] + e[1] * e[1]);
                }
                let (c, sn) = (s[0] + s[3], s[2] - s[1]);
                let th = sn.atan2(c);
                let scale = match mode {
                    PuppetMode::Rigid => 1.0,
                    PuppetMode::Normal => (c.hypot(sn) / norm.max(1e-12)).sqrt(),
                    PuppetMode::Distort => c.hypot(sn) / norm.max(1e-12),
                };
                let scale = if scale.is_finite() && scale > 0.0 {
                    scale
                } else {
                    1.0
                };
                let (sth, cth) = th.sin_cos();
                [scale * cth, -scale * sth, scale * sth, scale * cth]
            })
            .collect();
        for (k, tris) in self.pin_tris.iter().enumerate() {
            if let Some(Some(deg)) = fixed.get(k) {
                let (s, c) = deg.to_radians().sin_cos();
                for &t in tris {
                    rots[t] = [c, -s, s, c];
                }
            }
        }
        rots
    }

    fn global(&self, rots: &[[f64; 4]], pin_dst: &[[f64; 2]], v: &mut [[f64; 2]]) {
        let nv = v.len();
        let x = &self.mesh.verts;
        let mut bx = vec![0.0; nv];
        let mut by = vec![0.0; nv];
        for ((t, wt), r) in self.mesh.tris.iter().zip(&self.w).zip(rots) {
            for k in 0..3 {
                let (i, j) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                let e = [x[i][0] - x[j][0], x[i][1] - x[j][1]];
                let re = [r[0] * e[0] + r[1] * e[1], r[2] * e[0] + r[3] * e[1]];
                bx[i] += wt[k] * re[0];
                by[i] += wt[k] * re[1];
                bx[j] -= wt[k] * re[0];
                by[j] -= wt[k] * re[1];
            }
        }
        for (b, d) in self.bound.iter().zip(pin_dst) {
            for k in 0..3 {
                bx[b.verts[k]] += self.pin_weight * b.bary[k] * d[0];
                by[b.verts[k]] += self.pin_weight * b.bary[k] * d[1];
            }
        }
        for (i, &an) in self.anchored.iter().enumerate() {
            if an {
                bx[i] += self.pin_weight * x[i][0];
                by[i] += self.pin_weight * x[i][1];
            }
        }
        let mut sx: Vec<f64> = v.iter().map(|p| p[0]).collect();
        let mut sy: Vec<f64> = v.iter().map(|p| p[1]).collect();
        match &self.chol {
            Some(c) => {
                c.solve(&bx, &mut sx);
                c.solve(&by, &mut sy);
            }
            None => {
                self.matrix.solve(&bx, &mut sx);
                self.matrix.solve(&by, &mut sy);
            }
        }
        for (i, p) in v.iter_mut().enumerate() {
            *p = [sx[i], sy[i]];
        }
    }

    /// Deforms the mesh: `pin_dst` (one per pin given to [`Self::new`]), optional fixed pin
    /// rotations (degrees), `iterations` local/global rounds, warm-started from `init` when
    /// given. Returns the deformed vertices and the rigid ARAP energy after each round.
    pub fn solve(
        &self,
        pin_dst: &[[f64; 2]],
        rotations: &[Option<f64>],
        mode: PuppetMode,
        iterations: usize,
        init: Option<&[[f64; 2]]>,
    ) -> (Vec<[f64; 2]>, Vec<f64>) {
        let nv = self.mesh.verts.len();
        let mut v: Vec<[f64; 2]> = match init {
            Some(i) if i.len() == nv => i.to_vec(),
            _ => self.mesh.verts.clone(),
        };
        if nv == 0 {
            return (v, Vec::new());
        }
        if init.is_none_or(|i| i.len() != nv) {
            // Start from R = I, or with rotated pins, each triangle turned by the inverse-distance
            // weighted angle of the rotated pins (ARAP alone propagates a turn only slowly).
            let rotated: Vec<([f64; 2], f64)> = self
                .bound
                .iter()
                .zip(rotations)
                .filter_map(|(b, r)| {
                    r.map(|deg| {
                        (
                            (0..3).fold([0.0, 0.0], |a, k| {
                                [
                                    a[0] + b.bary[k] * self.mesh.verts[b.verts[k]][0],
                                    a[1] + b.bary[k] * self.mesh.verts[b.verts[k]][1],
                                ]
                            }),
                            deg.to_radians(),
                        )
                    })
                })
                .collect();
            let r: Vec<[f64; 4]> = self
                .mesh
                .tris
                .iter()
                .map(|t| {
                    if rotated.is_empty() {
                        return [1.0, 0.0, 0.0, 1.0];
                    }
                    let c = t.iter().fold([0.0, 0.0], |a, &i| {
                        [
                            a[0] + self.mesh.verts[i][0] / 3.0,
                            a[1] + self.mesh.verts[i][1] / 3.0,
                        ]
                    });
                    let (mut sw, mut sa) = (0.0, 0.0);
                    for (p, a) in &rotated {
                        let w = 1.0 / (dist2(*p, c) + 1.0);
                        sw += w;
                        sa += w * a;
                    }
                    let (s, co) = (sa / sw).sin_cos();
                    [co, -s, s, co]
                })
                .collect();
            self.global(&r, pin_dst, &mut v);
        }
        let mut energies = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let rots = self.local(&v, mode, rotations);
            self.global(&rots, pin_dst, &mut v);
            let rigid = self.local(&v, PuppetMode::Rigid, rotations);
            energies.push(self.energy_with(&v, &rigid, pin_dst));
        }
        (v, energies)
    }

    /// Energy of the current state with the rigid best fit (for monitoring).
    pub fn energy(&self, v: &[[f64; 2]], pin_dst: &[[f64; 2]], rotations: &[Option<f64>]) -> f64 {
        let rots = self.local(v, PuppetMode::Rigid, rotations);
        self.energy_with(v, &rots, pin_dst)
    }

    /// Triangle draw order (back to front) for pins with depths.
    pub fn draw_order(&self, depths: &[i32], pin_src: &[[f64; 2]]) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.mesh.tris.len()).collect();
        if depths.iter().all(|d| *d == 0) || pin_src.is_empty() {
            return order;
        }
        let depth_of = |t: usize| {
            let c = self.mesh.tris[t].iter().fold([0.0, 0.0], |a, &i| {
                [
                    a[0] + self.mesh.verts[i][0] / 3.0,
                    a[1] + self.mesh.verts[i][1] / 3.0,
                ]
            });
            let k = (0..pin_src.len())
                .min_by(|&a, &b| dist2(pin_src[a], c).total_cmp(&dist2(pin_src[b], c)))
                .unwrap_or(0);
            depths.get(k).copied().unwrap_or(0)
        };
        let d: Vec<i32> = order.iter().map(|&t| depth_of(t)).collect();
        order.sort_by_key(|&t| d[t]);
        order
    }
}
