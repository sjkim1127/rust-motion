//! Layouts, projections and motion models, their robust fitting, and image placement.

use crate::features::{self, Model};
use crate::photo_util::Rng;
use crate::transform::Homography;

/// Photomerge layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Auto,
    Perspective,
    Cylindrical,
    Spherical,
    Collage,
    Reposition,
}

impl Layout {
    pub fn parse(s: &str) -> Option<Layout> {
        Some(match s {
            "auto" => Layout::Auto,
            "perspective" => Layout::Perspective,
            "cylindrical" => Layout::Cylindrical,
            "spherical" => Layout::Spherical,
            "collage" => Layout::Collage,
            "reposition" => Layout::Reposition,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Layout::Auto => "auto",
            Layout::Perspective => "perspective",
            Layout::Cylindrical => "cylindrical",
            Layout::Spherical => "spherical",
            Layout::Collage => "collage",
            Layout::Reposition => "reposition",
        }
    }
    pub(super) fn projection(self) -> Projection {
        match self {
            Layout::Cylindrical => Projection::Cylinder,
            Layout::Spherical => Projection::Sphere,
            _ => Projection::Plane,
        }
    }
    pub(super) fn motion(self) -> Motion {
        match self {
            Layout::Auto | Layout::Perspective => Motion::Homography,
            Layout::Cylindrical | Layout::Spherical => Motion::Euclidean,
            Layout::Collage => Motion::Similarity,
            Layout::Reposition => Motion::Translation,
        }
    }
}

/// Surface an image is projected onto before its 2-D motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Plane,
    Cylinder,
    Sphere,
}

/// 2-D motion models in projected space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Translation,
    /// Rotation + translation.
    Euclidean,
    /// Rotation, uniform scale, translation.
    Similarity,
    Homography,
}

impl Motion {
    fn min_points(self) -> usize {
        match self {
            Motion::Translation => 1,
            Motion::Euclidean | Motion::Similarity => 2,
            Motion::Homography => 4,
        }
    }
    pub(super) fn n_params(self) -> usize {
        match self {
            Motion::Translation => 2,
            Motion::Euclidean => 3,
            Motion::Similarity => 4,
            Motion::Homography => 8,
        }
    }
    pub(super) fn to_params(self, h: &Homography) -> Vec<f64> {
        let m = h.0.map(|v| v / h.0[8]);
        match self {
            Motion::Translation => vec![m[2], m[5]],
            Motion::Euclidean => vec![m[3].atan2(m[0]), m[2], m[5]],
            Motion::Similarity => vec![m[0], m[3], m[2], m[5]],
            Motion::Homography => m[..8].to_vec(),
        }
    }
    pub(super) fn to_homography(self, p: &[f64]) -> Homography {
        match self {
            Motion::Translation => Homography([1.0, 0.0, p[0], 0.0, 1.0, p[1], 0.0, 0.0, 1.0]),
            Motion::Euclidean => {
                let (s, c) = p[0].sin_cos();
                Homography([c, -s, p[1], s, c, p[2], 0.0, 0.0, 1.0])
            }
            Motion::Similarity => Homography([p[0], -p[1], p[2], p[1], p[0], p[3], 0.0, 0.0, 1.0]),
            Motion::Homography => Homography([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], 1.0]),
        }
    }
}

/// Least-squares fit of `motion` mapping `src[i]` → `dst[i]`.
pub fn fit_motion(motion: Motion, src: &[[f64; 2]], dst: &[[f64; 2]]) -> Option<Homography> {
    match motion {
        Motion::Translation => features::fit(Model::Translation, src, dst),
        Motion::Similarity => features::fit(Model::Similarity, src, dst),
        Motion::Homography => features::fit(Model::Homography, src, dst),
        Motion::Euclidean => {
            let n = src.len();
            if n < 2 {
                return None;
            }
            let nf = n as f64;
            let cs = src
                .iter()
                .fold([0.0, 0.0], |a, p| [a[0] + p[0] / nf, a[1] + p[1] / nf]);
            let cd = dst
                .iter()
                .fold([0.0, 0.0], |a, p| [a[0] + p[0] / nf, a[1] + p[1] / nf]);
            let (mut sxx, mut sxy) = (0.0, 0.0);
            for (s, d) in src.iter().zip(dst) {
                let (a, b) = ([s[0] - cs[0], s[1] - cs[1]], [d[0] - cd[0], d[1] - cd[1]]);
                sxx += a[0] * b[0] + a[1] * b[1];
                sxy += a[0] * b[1] - a[1] * b[0];
            }
            let th = sxy.atan2(sxx);
            let (s, c) = th.sin_cos();
            Some(Homography([
                c,
                -s,
                cd[0] - (c * cs[0] - s * cs[1]),
                s,
                c,
                cd[1] - (s * cs[0] + c * cs[1]),
                0.0,
                0.0,
                1.0,
            ]))
        }
    }
}

/// RANSAC over [`Motion`] models; returns the refit model and its inlier indices.
pub fn ransac_motion(
    motion: Motion,
    src: &[[f64; 2]],
    dst: &[[f64; 2]],
    iters: usize,
    threshold: f64,
    seed: u64,
) -> Option<(Homography, Vec<usize>)> {
    let k = motion.min_points();
    let n = src.len();
    if n < k {
        return None;
    }
    let mut rng = Rng(seed ^ 0x5EED_0FA1);
    let inliers_of = |h: &Homography| -> Vec<usize> {
        (0..n)
            .filter(|&i| {
                let (x, y) = h.apply(src[i][0], src[i][1]);
                x.is_finite() && (x - dst[i][0]).hypot(y - dst[i][1]) <= threshold
            })
            .collect()
    };
    let mut best: Option<(Homography, Vec<usize>)> = None;
    for _ in 0..iters.max(1) {
        let mut idx: Vec<usize> = Vec::with_capacity(k);
        if n == k {
            idx = (0..k).collect();
        }
        while idx.len() < k {
            let i = rng.below(n);
            if !idx.contains(&i) {
                idx.push(i);
            }
        }
        let (s, d): (Vec<[f64; 2]>, Vec<[f64; 2]>) = idx.iter().map(|&i| (src[i], dst[i])).unzip();
        let Some(h) = fit_motion(motion, &s, &d) else {
            continue;
        };
        let inl = inliers_of(&h);
        if best.as_ref().is_none_or(|(_, b)| inl.len() > b.len()) {
            best = Some((h, inl));
        }
    }
    let (h, inl) = best?;
    if inl.len() < k {
        return None;
    }
    let (s, d): (Vec<[f64; 2]>, Vec<[f64; 2]>) = inl.iter().map(|&i| (src[i], dst[i])).unzip();
    let h2 = fit_motion(motion, &s, &d).unwrap_or(h);
    let inl2 = inliers_of(&h2);
    Some(if inl2.len() >= inl.len() {
        (h2, inl2)
    } else {
        (h, inl)
    })
}

/// Where one image lands in the panorama: image px → (undistort) → projection → 2-D motion.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub projection: Projection,
    /// Focal length in px (the projection radius).
    pub focal: f64,
    /// Projection centre in image px (the image centre).
    pub center: [f64; 2],
    /// Radial distortion `k1` on radii normalised by the focal length (0 = none).
    pub k1: f64,
    /// Projected coordinates → panorama coordinates.
    pub h: Homography,
}

impl Placement {
    /// Image px → projected coordinates (centred, in px of the focal length).
    pub fn project(&self, x: f64, y: f64) -> [f64; 2] {
        let (mut dx, mut dy) = (x - self.center[0], y - self.center[1]);
        if self.k1 != 0.0 {
            let s = 1.0 + self.k1 * (dx * dx + dy * dy) / (self.focal * self.focal);
            dx *= s;
            dy *= s;
        }
        let f = self.focal;
        match self.projection {
            Projection::Plane => [dx, dy],
            Projection::Cylinder => [f * dx.atan2(f), f * dy / dx.hypot(f)],
            Projection::Sphere => [f * dx.atan2(f), f * dy.atan2(dx.hypot(f))],
        }
    }

    /// Projected → image px (`None` outside the valid hemisphere).
    pub fn unproject(&self, a: f64, b: f64) -> Option<(f64, f64)> {
        let f = self.focal;
        let (mut dx, mut dy) = match self.projection {
            Projection::Plane => (a, b),
            Projection::Cylinder => {
                let th = a / f;
                if th.abs() >= 1.55 {
                    return None;
                }
                (f * th.tan(), b / th.cos())
            }
            Projection::Sphere => {
                let (th, ph) = (a / f, b / f);
                if th.abs() >= 1.55 || ph.abs() >= 1.55 {
                    return None;
                }
                (f * th.tan(), f * ph.tan() / th.cos())
            }
        };
        if self.k1 != 0.0 {
            // Invert r_u = r_d (1 + k1 r_d² / f²) by Newton steps on r_d.
            let ru = dx.hypot(dy);
            if ru > 1e-9 {
                let mut rd = ru;
                for _ in 0..8 {
                    let g = rd * (1.0 + self.k1 * rd * rd / (f * f)) - ru;
                    let dg = 1.0 + 3.0 * self.k1 * rd * rd / (f * f);
                    if dg.abs() < 1e-9 {
                        break;
                    }
                    rd -= g / dg;
                }
                if !(rd.is_finite() && rd > 0.0) {
                    return None;
                }
                dx *= rd / ru;
                dy *= rd / ru;
            }
        }
        Some((dx + self.center[0], dy + self.center[1]))
    }

    /// Image px → panorama px.
    pub fn forward(&self, x: f64, y: f64) -> (f64, f64) {
        let p = self.project(x, y);
        self.h.apply(p[0], p[1])
    }

    /// Panorama px → image px.
    pub fn inverse(&self, u: f64, v: f64) -> Option<(f64, f64)> {
        let inv = self.h.inverse()?;
        let m = &inv.0;
        let w = m[6] * u + m[7] * v + m[8];
        if w <= 1e-12 {
            return None;
        }
        let (a, b) = inv.apply(u, v);
        self.unproject(a, b)
    }

    /// The same placement for images `k×` larger (registration ran at `1/k` scale).
    pub fn scaled(&self, k: f64) -> Placement {
        let s = Homography([k, 0.0, 0.0, 0.0, k, 0.0, 0.0, 0.0, 1.0]);
        let si = Homography([1.0 / k, 0.0, 0.0, 0.0, 1.0 / k, 0.0, 0.0, 0.0, 1.0]);
        Placement {
            projection: self.projection,
            focal: self.focal * k,
            center: [self.center[0] * k, self.center[1] * k],
            k1: self.k1,
            h: s.mul(&self.h).mul(&si),
        }
    }

    /// Shifted in the panorama.
    pub fn translated(&self, dx: f64, dy: f64) -> Placement {
        let t = Homography([1.0, 0.0, dx, 0.0, 1.0, dy, 0.0, 0.0, 1.0]);
        Placement {
            h: t.mul(&self.h),
            ..self.clone()
        }
    }

    /// Bounds `[x0, y0, x1, y1]` of a `w × h` image in the panorama (sampled along its edges).
    pub fn bounds(&self, w: f64, h: f64) -> [f64; 4] {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        let n = 32;
        for i in 0..=n {
            let t = i as f64 / n as f64;
            for (x, y) in [(t * w, 0.0), (t * w, h), (0.0, t * h), (w, t * h)] {
                let (u, v) = self.forward(x, y);
                if u.is_finite() && v.is_finite() {
                    b = [b[0].min(u), b[1].min(v), b[2].max(u), b[3].max(v)];
                }
            }
        }
        b
    }
}
