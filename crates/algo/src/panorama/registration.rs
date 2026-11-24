//! Registration: the chained alignment of all images and its bundle adjustment.

use super::*;
use crate::photo_util::{par_map, solve_dense};
use crate::transform::Homography;

/// Options for [`align`].
#[derive(Clone, Debug)]
pub struct AlignOptions {
    pub layout: Layout,
    /// Focal length in registration px (EXIF); estimated when `None`.
    pub focal: Option<f64>,
    /// Image that stays put (`None`: the best-connected one).
    pub reference: Option<usize>,
    /// Refine a radial distortion coefficient (Geometric Distortion Correction).
    pub geometric: bool,
}

/// Result of [`align`].
#[derive(Clone, Debug)]
pub struct Alignment {
    /// The layout actually used (Auto resolved).
    pub layout: Layout,
    pub reference: usize,
    /// Per image; `None` for images that couldn't be connected.
    pub placements: Vec<Option<Placement>>,
    pub focal: f64,
    pub k1: f64,
    /// Verified pairs `(a, b, inliers)`.
    pub pairs: Vec<(usize, usize, usize)>,
    /// RMS reprojection error (px, registration scale) after bundle adjustment.
    pub rms: f64,
}

struct PairData {
    pub(super) a: usize,
    pub(super) b: usize,
    pa: Vec<[f64; 2]>,
    pb: Vec<[f64; 2]>,
}

fn base_placement(f: &ImageFeatures, projection: Projection, focal: f64, k1: f64) -> Placement {
    Placement {
        projection,
        focal,
        center: [f.w as f64 / 2.0, f.h as f64 / 2.0],
        k1,
        h: Homography::IDENTITY,
    }
}

/// Aligns images from their features. `None` when fewer than two images connect.
pub fn align(
    feats: &[ImageFeatures],
    matches: &[PairMatch],
    opts: &AlignOptions,
) -> Option<Alignment> {
    let n = feats.len();
    if n < 2 || matches.is_empty() {
        return None;
    }
    let focal = opts.focal.filter(|f| *f > 1.0).unwrap_or_else(|| {
        let mut fs: Vec<f64> = matches
            .iter()
            .filter_map(|m| {
                let (fa, fb) = (&feats[m.a], &feats[m.b]);
                focal_from_homography(
                    &m.homography,
                    [fa.w as f64 / 2.0, fa.h as f64 / 2.0],
                    [fb.w as f64 / 2.0, fb.h as f64 / 2.0],
                )
            })
            .filter(|f| f.is_finite())
            .collect();
        let side = feats.iter().map(|f| f.w.max(f.h)).max().unwrap_or(1) as f64;
        fs.retain(|f| *f > side * 0.2 && *f < side * 20.0);
        fs.sort_by(f64::total_cmp);
        if fs.is_empty() {
            side
        } else {
            fs[fs.len() / 2]
        }
    });
    if opts.layout != Layout::Auto {
        return align_layout(feats, matches, opts, opts.layout, focal);
    }
    // Auto: perspective unless it is too stretched (Photoshop's rule of thumb), then cylindrical,
    // or spherical when the panorama is also tall.
    if let Some(a) = align_layout(feats, matches, opts, Layout::Perspective, focal) {
        if !too_stretched(feats, &a) {
            return Some(Alignment {
                layout: Layout::Perspective,
                ..a
            });
        }
    }
    let cyl = align_layout(feats, matches, opts, Layout::Cylindrical, focal)?;
    let (mut y0, mut y1) = (f64::MAX, f64::MIN);
    for (p, f) in cyl.placements.iter().zip(feats) {
        if let Some(p) = p {
            let b = p.bounds(f.w as f64, f.h as f64);
            y0 = y0.min(b[1]);
            y1 = y1.max(b[3]);
        }
    }
    if (y1 - y0) / cyl.focal > 1.4 {
        return align_layout(feats, matches, opts, Layout::Spherical, cyl.focal);
    }
    Some(cyl)
}

/// A perspective panorama is too stretched when an image grows or shrinks a lot, flips, or
/// the field of view exceeds about 110°.
fn too_stretched(feats: &[ImageFeatures], a: &Alignment) -> bool {
    let (mut x0, mut x1) = (f64::MAX, f64::MIN);
    for (p, f) in a.placements.iter().zip(feats) {
        let Some(p) = p else { continue };
        let (w, h) = (f.w as f64, f.h as f64);
        let c: Vec<(f64, f64)> = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
            .iter()
            .map(|&(x, y)| p.forward(x, y))
            .collect();
        // Signed area (shoelace); orientation must be preserved.
        let area: f64 = (0..4)
            .map(|i| c[i].0 * c[(i + 1) % 4].1 - c[(i + 1) % 4].0 * c[i].1)
            .sum::<f64>()
            / 2.0;
        let ratio = area / (w * h);
        if !ratio.is_finite() || !(0.3..=3.0).contains(&ratio) {
            return true;
        }
        let m = &p.h.0;
        for &(x, y) in &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
            let q = p.project(x, y);
            if m[6] * q[0] + m[7] * q[1] + m[8] <= 0.0 {
                return true;
            }
        }
        for (u, _) in &c {
            x0 = x0.min(*u);
            x1 = x1.max(*u);
        }
    }
    // Horizontal field of view through the reference's focal length.
    let fov = 2.0 * ((x1 - x0) / 2.0 / a.focal).atan();
    fov > 110f64.to_radians()
}

fn align_layout(
    feats: &[ImageFeatures],
    matches: &[PairMatch],
    opts: &AlignOptions,
    layout: Layout,
    focal: f64,
) -> Option<Alignment> {
    let n = feats.len();
    let projection = layout.projection();
    let motion = layout.motion();
    // Pairwise motions in projected space.
    let mut pairs: Vec<PairData> = Vec::new();
    let mut edges: Vec<(usize, usize, usize, Homography)> = Vec::new();
    for m in matches {
        let (pa, pb) = (
            base_placement(&feats[m.a], projection, focal, 0.0),
            base_placement(&feats[m.b], projection, focal, 0.0),
        );
        let qa: Vec<[f64; 2]> = m.pts_a.iter().map(|p| pa.project(p[0], p[1])).collect();
        let qb: Vec<[f64; 2]> = m.pts_b.iter().map(|p| pb.project(p[0], p[1])).collect();
        let thr = (feats[m.a].w.max(feats[m.a].h) as f64 / 250.0).clamp(2.0, 5.0);
        let Some((hm, inl)) = ransac_motion(motion, &qb, &qa, 600, thr, (m.a * 977 + m.b) as u64)
        else {
            continue;
        };
        if inl.len() < 8 || (inl.len() as f64) < 0.5 * m.pts_a.len() as f64 {
            continue;
        }
        edges.push((m.a, m.b, inl.len(), hm));
        // Bundle adjustment uses at most 80 evenly spread inliers per pair.
        let step = inl.len().div_ceil(80).max(1);
        let sel: Vec<usize> = inl.iter().step_by(step).copied().collect();
        pairs.push(PairData {
            a: m.a,
            b: m.b,
            pa: sel.iter().map(|&i| m.pts_a[i]).collect(),
            pb: sel.iter().map(|&i| m.pts_b[i]).collect(),
        });
    }
    if edges.is_empty() {
        return None;
    }
    let mut degree = vec![0usize; n];
    for e in &edges {
        degree[e.0] += e.2;
        degree[e.1] += e.2;
    }
    let reference = opts.reference.filter(|r| *r < n).unwrap_or_else(|| {
        // Best connected; ties go to the middle image.
        (0..n)
            .max_by(|&i, &j| {
                degree[i].cmp(&degree[j]).then_with(|| {
                    (j as i64 - n as i64 / 2)
                        .abs()
                        .cmp(&(i as i64 - n as i64 / 2).abs())
                })
            })
            .unwrap_or(0)
    });
    // Maximum spanning tree (Prim) from the reference; transforms map projected → panorama.
    let mut tf: Vec<Option<Homography>> = vec![None; n];
    let rc = [
        feats[reference].w as f64 / 2.0,
        feats[reference].h as f64 / 2.0,
    ];
    tf[reference] = Some(Homography([
        1.0, 0.0, rc[0], 0.0, 1.0, rc[1], 0.0, 0.0, 1.0,
    ]));
    loop {
        let mut best: Option<(usize, usize, Homography)> = None;
        for e in &edges {
            // e.3 maps b → a.
            let cand = match (tf[e.0], tf[e.1]) {
                (Some(ta), None) => Some((e.1, ta.mul(&e.3))),
                (None, Some(tb)) => e.3.inverse().map(|inv| (e.0, tb.mul(&inv))),
                _ => None,
            };
            if let Some((node, t)) = cand {
                if best.as_ref().is_none_or(|b| e.2 > b.1) {
                    best = Some((node, e.2, t));
                }
            }
        }
        let Some((node, _, t)) = best else { break };
        tf[node] = Some(t);
    }
    let placed: Vec<usize> = (0..n).filter(|&i| tf[i].is_some()).collect();
    if placed.len() < 2 {
        return None;
    }
    // Bundle adjustment (Levenberg–Marquardt, numeric Jacobian).
    let movable: Vec<usize> = placed.iter().copied().filter(|&i| i != reference).collect();
    let mp = motion.n_params();
    let refine_focal = projection != Projection::Plane;
    let n_glob = usize::from(opts.geometric) + usize::from(refine_focal);
    let mut params: Vec<f64> = movable
        .iter()
        .filter_map(|&i| tf[i].as_ref())
        .flat_map(|t| motion.to_params(t))
        .collect();
    if opts.geometric {
        params.push(0.0);
    }
    if refine_focal {
        params.push(focal.ln());
    }
    let n_par = params.len();
    let decode = |p: &[f64]| -> Vec<Option<Placement>> {
        let k1 = if opts.geometric {
            p[movable.len() * mp]
        } else {
            0.0
        };
        let f = if refine_focal {
            p[n_par - 1].exp()
        } else {
            focal
        };
        (0..n)
            .map(|i| {
                let h = if i == reference {
                    tf[reference]?
                } else {
                    let k = movable.iter().position(|&m| m == i)?;
                    motion.to_homography(&p[k * mp..(k + 1) * mp])
                };
                Some(Placement {
                    h,
                    ..base_placement(&feats[i], projection, f, k1)
                })
            })
            .collect()
    };
    let residuals = |p: &[f64]| -> Vec<f64> {
        let pl = decode(p);
        let mut r = Vec::new();
        for pd in &pairs {
            let (Some(a), Some(b)) = (&pl[pd.a], &pl[pd.b]) else {
                continue;
            };
            for (qa, qb) in pd.pa.iter().zip(&pd.pb) {
                let (ua, va) = a.forward(qa[0], qa[1]);
                let (ub, vb) = b.forward(qb[0], qb[1]);
                r.push(ua - ub);
                r.push(va - vb);
            }
        }
        // Soft priors keep the global terms sane when few pairs constrain them.
        if opts.geometric {
            r.push(p[movable.len() * mp] * 20.0);
        }
        if refine_focal {
            r.push((p[n_par - 1] - focal.ln()) * 2.0);
        }
        r
    };
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let mut cur = residuals(&params);
    let mut lambda = 1e-3;
    if n_par > 0 && !movable.is_empty() {
        for _ in 0..40 {
            let m_res = cur.len();
            let cols: Vec<Vec<f64>> = par_map(n_par, |j| {
                let step = 1e-6 * (1.0 + params[j].abs());
                let mut pp = params.clone();
                pp[j] += step;
                let rp = residuals(&pp);
                pp[j] -= 2.0 * step;
                let rm = residuals(&pp);
                (0..m_res).map(|i| (rp[i] - rm[i]) / (2.0 * step)).collect()
            });
            let mut jtj = vec![0.0; n_par * n_par];
            let mut jtr = vec![0.0; n_par];
            for a in 0..n_par {
                jtr[a] = -cols[a].iter().zip(&cur).map(|(j, r)| j * r).sum::<f64>();
                for b in a..n_par {
                    let v: f64 = cols[a].iter().zip(&cols[b]).map(|(x, y)| x * y).sum();
                    jtj[a * n_par + b] = v;
                    jtj[b * n_par + a] = v;
                }
            }
            let mut improved = false;
            for _ in 0..8 {
                let mut a = jtj.clone();
                for d in 0..n_par {
                    a[d * n_par + d] += lambda * (jtj[d * n_par + d] + 1e-9);
                }
                let Some(delta) = solve_dense(n_par, a, jtr.clone()) else {
                    lambda *= 10.0;
                    continue;
                };
                let trial: Vec<f64> = params.iter().zip(&delta).map(|(p, d)| p + d).collect();
                let rt = residuals(&trial);
                if cost(&rt) < cost(&cur) {
                    let gain = cost(&cur) - cost(&rt);
                    params = trial;
                    cur = rt;
                    lambda = (lambda * 0.3).max(1e-9);
                    improved = gain > 1e-10 * cost(&cur).max(1e-12);
                    break;
                }
                lambda *= 10.0;
            }
            if !improved {
                break;
            }
        }
    }
    let placements = decode(&params);
    let n_res = pairs.iter().map(|p| p.pa.len()).sum::<usize>().max(1);
    let rms = (cost(&cur[..(cur.len() - n_glob).min(cur.len())]) / n_res as f64).sqrt();
    let k1 = if opts.geometric {
        params[movable.len() * mp]
    } else {
        0.0
    };
    let focal = if refine_focal {
        params[n_par - 1].exp()
    } else {
        focal
    };
    Some(Alignment {
        layout,
        reference,
        placements,
        focal,
        k1,
        pairs: edges.iter().map(|e| (e.0, e.1, e.2)).collect(),
        rms,
    })
}
