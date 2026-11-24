//! Feature detection, pair matching and focal-length estimation.

use super::*;
use crate::features::{self, Descriptor};
use crate::photo_util::par_map;
use crate::transform::Homography;

/// Keypoints and descriptors of one image (registration scale).
#[derive(Clone, Debug, Default)]
pub struct ImageFeatures {
    pub w: usize,
    pub h: usize,
    pub points: Vec<[f64; 2]>,
    pub desc: Vec<Descriptor>,
}

/// Detects up to `max` corners of a `w × h` luminance image and describes them.
pub fn detect(
    w: usize,
    h: usize,
    luma: &[f32],
    valid: Option<&[bool]>,
    max: usize,
) -> ImageFeatures {
    let min_dist = (w.max(h) as f32 / 70.0).clamp(3.0, 20.0);
    let corners = features::harris(w, h, luma, max, min_dist, valid);
    let desc = features::describe(w, h, luma, &corners, false);
    ImageFeatures {
        w,
        h,
        points: corners.iter().map(|c| [c.x as f64, c.y as f64]).collect(),
        desc,
    }
}

/// One image for [`detect_many`]: `(w, h, luma, valid)`.
pub type PreparedImage = (usize, usize, Vec<f32>, Option<Vec<bool>>);

/// [`detect`] for several images `(w, h, luma, valid)` in parallel.
pub fn detect_many(imgs: &[PreparedImage], max: usize) -> Vec<ImageFeatures> {
    par_map(imgs.len(), |i| {
        let (w, h, l, v) = &imgs[i];
        detect(*w, *h, l, v.as_deref(), max)
    })
}

/// A verified image pair: point correspondences (image `a` px, image `b` px).
#[derive(Clone, Debug)]
pub struct PairMatch {
    pub a: usize,
    pub b: usize,
    pub pts_a: Vec<[f64; 2]>,
    pub pts_b: Vec<[f64; 2]>,
    /// Homography `b → a` in image px.
    pub homography: Homography,
}

/// Matches every pair and keeps those that pass RANSAC + the Brown–Lowe verification.
pub fn match_pairs(feats: &[ImageFeatures]) -> Vec<PairMatch> {
    let n = feats.len();
    let pairs: Vec<(usize, usize)> = (0..n)
        .flat_map(|a| (a + 1..n).map(move |b| (a, b)))
        .collect();
    let out = par_map(pairs.len(), |k| {
        let (a, b) = pairs[k];
        let (fa, fb) = (&feats[a], &feats[b]);
        if fa.points.len() < 6 || fb.points.len() < 6 {
            return None;
        }
        let m = features::match_descriptors(&fb.desc, &fa.desc, 0.8);
        if m.len() < 8 {
            return None;
        }
        let src: Vec<[f64; 2]> = m.iter().map(|&(i, _)| fb.points[i]).collect();
        let dst: Vec<[f64; 2]> = m.iter().map(|&(_, j)| fa.points[j]).collect();
        let thr = (fa.w.max(fa.h) as f64 / 300.0).clamp(1.5, 4.0);
        let (hm, inl) = ransac_motion(
            Motion::Homography,
            &src,
            &dst,
            1200,
            thr,
            (a * 131 + b) as u64,
        )?;
        if (inl.len() as f64) < 8.0 + 0.3 * m.len() as f64 || inl.len() < 10 {
            return None;
        }
        Some(PairMatch {
            a,
            b,
            pts_a: inl.iter().map(|&i| dst[i]).collect(),
            pts_b: inl.iter().map(|&i| src[i]).collect(),
            homography: hm,
        })
    });
    out.into_iter().flatten().collect()
}

/// Focal length (px) of a rotating camera from a homography between two of its images (image
/// px, both centred at `c`), or `None` when the homography doesn't constrain it.
pub fn focal_from_homography(h: &Homography, ca: [f64; 2], cb: [f64; 2]) -> Option<f64> {
    let ta = Homography([1.0, 0.0, -ca[0], 0.0, 1.0, -ca[1], 0.0, 0.0, 1.0]);
    let tb = Homography([1.0, 0.0, cb[0], 0.0, 1.0, cb[1], 0.0, 0.0, 1.0]);
    let m = ta.mul(h).mul(&tb).0;
    let pick = |c1: (f64, f64), c2: (f64, f64)| -> Option<f64> {
        // Each candidate is (numerator, denominator) of f²; prefer the better-conditioned one.
        let ok = |(n, d): (f64, f64)| (d.abs() > 1e-12 && n / d > 0.0).then_some((n / d, d.abs()));
        match (ok(c1), ok(c2)) {
            (Some(a), Some(b)) => Some(if a.1 >= b.1 { a.0 } else { b.0 }),
            (Some(a), None) | (None, Some(a)) => Some(a.0),
            _ => None,
        }
    };
    // Columns of K⁻¹HK orthogonal and of equal norm (focal of b) …
    let f1 = pick(
        (-(m[0] * m[1] + m[3] * m[4]), m[6] * m[7]),
        (
            m[1] * m[1] + m[4] * m[4] - m[0] * m[0] - m[3] * m[3],
            m[6] * m[6] - m[7] * m[7],
        ),
    );
    // … rows likewise (focal of a).
    let f0 = pick(
        (-(m[2] * m[5]), m[0] * m[3] + m[1] * m[4]),
        (
            m[5] * m[5] - m[2] * m[2],
            m[0] * m[0] + m[1] * m[1] - m[3] * m[3] - m[4] * m[4],
        ),
    );
    match (f0, f1) {
        (Some(a), Some(b)) => Some((a * b).sqrt().sqrt()),
        (Some(a), None) | (None, Some(a)) => Some(a.sqrt()),
        _ => None,
    }
}
