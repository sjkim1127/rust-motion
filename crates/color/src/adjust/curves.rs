//! Curves: control-point handling and lookup-table generation.

use super::*;

/// Whether `points` is the straight 0→0, 1→1 line (or too short to be a curve).
pub fn is_identity_curve(points: &[CurvePoint]) -> bool {
    match points {
        [a, b] => a.input == 0.0 && a.output == 0.0 && b.input == 1.0 && b.output == 1.0,
        p => p.len() < 2,
    }
}

/// Entries in a pencil-drawn Curves map (one per 8-bit level, as in PSD `curv` maps).
pub const CURVE_MAP_LEN: usize = 256;

/// A pencil-mode curve from its map: `outputs[k]` (0..=1) is the output at input `k / 255`.
/// Missing entries repeat the last one; extra entries are ignored.
pub fn curve_map(outputs: &[f32]) -> Vec<CurvePoint> {
    let last = (CURVE_MAP_LEN - 1) as f32;
    let mut prev = 0.0;
    (0..CURVE_MAP_LEN)
        .map(|k| {
            let out = outputs
                .get(k)
                .copied()
                .filter(|v| v.is_finite())
                .map_or(prev, |v| v.clamp(0.0, 1.0));
            prev = out;
            CurvePoint {
                input: k as f32 / last,
                output: out,
            }
        })
        .collect()
}

/// The [`CURVE_MAP_LEN`] outputs (0..=1) of `points` read as a pencil-mode curve: straight
/// segments between the points, constant outside the first/last (identity without two points).
pub fn curve_map_outputs(points: &[CurvePoint]) -> Vec<f32> {
    let mut pts: Vec<(f32, f32)> = points
        .iter()
        .filter(|p| p.input.is_finite() && p.output.is_finite())
        .map(|p| (p.input, p.output))
        .collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let last = (CURVE_MAP_LEN - 1) as f32;
    (0..CURVE_MAP_LEN)
        .map(|k| {
            let x = k as f32 / last;
            let y = match (pts.first(), pts.last()) {
                (Some(a), Some(b)) if pts.len() >= 2 => {
                    if x <= a.0 {
                        a.1
                    } else if x >= b.0 {
                        b.1
                    } else {
                        let i = pts.partition_point(|p| p.0 < x);
                        match (pts.get(i.wrapping_sub(1)), pts.get(i)) {
                            (Some(p0), Some(p1)) if p1.0 > p0.0 => {
                                p0.1 + (p1.1 - p0.1) * (x - p0.0) / (p1.0 - p0.0)
                            }
                            (_, Some(p1)) => p1.1,
                            _ => b.1,
                        }
                    }
                }
                _ => x,
            };
            y.clamp(0.0, 1.0)
        })
        .collect()
}

/// The LUT of one Curves curve: [`curve_map_lut`] for a pencil-drawn map, else [`curve_lut`].
pub fn curve_lut_mode(points: &[CurvePoint], pencil: bool) -> Vec<f32> {
    if pencil {
        curve_map_lut(points)
    } else {
        curve_lut(points)
    }
}

/// A pencil-drawn Curves map (see [`curve_map`]): straight segments
/// between its entries, constant outside the first/last, clamped to 0..=1, sampled into a LUT.
pub fn curve_map_lut(points: &[CurvePoint]) -> Vec<f32> {
    let mut pts: Vec<(f32, f32)> = points
        .iter()
        .filter(|p| p.input.is_finite() && p.output.is_finite())
        .map(|p| (p.input, p.output))
        .collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);
    let ident = |k: usize| k as f32 / (LUT_SIZE - 1) as f32;
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else {
        return (0..LUT_SIZE).map(ident).collect();
    };
    let mut seg = 0;
    (0..LUT_SIZE)
        .map(|k| {
            let xv = ident(k);
            if xv <= first.0 {
                return first.1.clamp(0.0, 1.0);
            }
            if xv >= last.0 {
                return last.1.clamp(0.0, 1.0);
            }
            while pts.get(seg + 1).is_some_and(|p| p.0 < xv) {
                seg += 1;
            }
            let (Some(&(x0, y0)), Some(&(x1, y1))) = (pts.get(seg), pts.get(seg + 1)) else {
                return last.1.clamp(0.0, 1.0);
            };
            let t = if x1 > x0 { (xv - x0) / (x1 - x0) } else { 0.0 };
            (y0 + (y1 - y0) * t).clamp(0.0, 1.0)
        })
        .collect()
}

/// Photoshop curve: a natural cubic spline through the points (it may
/// overshoot between points, as Photoshop's does), constant outside the
/// first/last point, clamped to 0..=1, sampled into a LUT.
pub fn curve_lut(points: &[CurvePoint]) -> Vec<f32> {
    let mut pts: Vec<(f32, f32)> = points.iter().map(|p| (p.input, p.output)).collect();
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);
    if pts.len() < 2 {
        return (0..LUT_SIZE)
            .map(|i| i as f32 / (LUT_SIZE - 1) as f32)
            .collect();
    }
    let n = pts.len();
    // Second derivatives of the natural spline (tridiagonal solve).
    let mut m2 = vec![0.0f64; n];
    if n > 2 {
        let x: Vec<f64> = pts.iter().map(|p| f64::from(p.0)).collect();
        let y: Vec<f64> = pts.iter().map(|p| f64::from(p.1)).collect();
        let mut c = vec![0.0f64; n];
        let mut d = vec![0.0f64; n];
        for i in 1..n - 1 {
            let (h0, h1) = (x[i] - x[i - 1], x[i + 1] - x[i]);
            let a = h0 / 6.0;
            let b = (h0 + h1) / 3.0;
            let cc = h1 / 6.0;
            let r = (y[i + 1] - y[i]) / h1 - (y[i] - y[i - 1]) / h0;
            let denom = b - a * c[i - 1];
            c[i] = cc / denom;
            d[i] = (r - a * d[i - 1]) / denom;
        }
        for i in (1..n - 1).rev() {
            m2[i] = d[i] - c[i] * m2[i + 1];
        }
    }
    (0..LUT_SIZE)
        .map(|k| {
            let xv = k as f32 / (LUT_SIZE - 1) as f32;
            if xv <= pts[0].0 {
                return pts[0].1.clamp(0.0, 1.0);
            }
            if xv >= pts[n - 1].0 {
                return pts[n - 1].1.clamp(0.0, 1.0);
            }
            let i = pts.windows(2).position(|w| xv <= w[1].0).unwrap_or(n - 2);
            let (x0, y0) = (f64::from(pts[i].0), f64::from(pts[i].1));
            let (x1, y1) = (f64::from(pts[i + 1].0), f64::from(pts[i + 1].1));
            let h = x1 - x0;
            let t = f64::from(xv);
            let a = (x1 - t) / h;
            let b = (t - x0) / h;
            let y = a * y0
                + b * y1
                + ((a * a * a - a) * m2[i] + (b * b * b - b) * m2[i + 1]) * h * h / 6.0;
            (y as f32).clamp(0.0, 1.0)
        })
        .collect()
}

/// A layer-style contour curve: [`curve_lut`], except that the curve bends sharply at corner
/// points (`corners[i]` true; Photoshop's Contour Editor "Corner"): each run between corners is
/// its own natural spline, so all-corner points make a polyline. Missing flags are smooth.
pub fn contour_curve_lut(points: &[CurvePoint], corners: &[bool]) -> Vec<f32> {
    let mut pts: Vec<(CurvePoint, bool)> = points
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), corners.get(i).copied().unwrap_or(false)))
        .collect();
    if !pts.iter().any(|p| p.1) {
        return curve_lut(points);
    }
    pts.sort_by(|a, b| {
        a.0.input
            .partial_cmp(&b.0.input)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    pts.dedup_by(|a, b| (a.0.input - b.0.input).abs() < 1e-6);
    let mut out: Vec<f32> = Vec::new();
    let mut start = 0usize;
    let last = pts.len().saturating_sub(1);
    for i in 1..=last {
        let corner = pts.get(i).is_some_and(|p| p.1);
        if !(corner || i == last) {
            continue;
        }
        let Some(seg) = pts.get(start..=i) else { break };
        let from = seg.first().map_or(0.0, |p| p.0.input);
        let lut = curve_lut(&seg.iter().map(|p| p.0.clone()).collect::<Vec<_>>());
        if out.is_empty() {
            out = lut;
        } else {
            for (k, (o, v)) in out.iter_mut().zip(lut).enumerate() {
                if k as f32 / (LUT_SIZE - 1) as f32 >= from {
                    *o = v;
                }
            }
        }
        start = i;
    }
    if out.is_empty() {
        curve_lut(points)
    } else {
        out
    }
}
