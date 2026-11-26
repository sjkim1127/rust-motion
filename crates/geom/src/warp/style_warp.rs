//! A preset style prepared for one box: its control grid and its map.

use super::*;

/// A preset style prepared for one box: its control grid and the box it bends.
#[derive(Clone, Copy, Debug)]
pub struct StyleWarp {
    /// The 4 × 4 Bezier control grid in box units (0..1, y down), row-major.
    grid: Grid,
    /// Box origin and size.
    o: (f64, f64),
    size: (f64, f64),
}

/// A single bicubic Bezier patch's control points, `[row][column]`, in box units.
type Grid = [[[f64; 2]; 4]; 4];

impl StyleWarp {
    /// Prepares `style` with bend / distortions in percent (-100..100) for `[x0, y0, x1, y1]`.
    /// `None` for non-presets, an all-zero warp or an empty box.
    pub fn new(
        style: WarpStyle,
        bend: f64,
        h_distort: f64,
        v_distort: f64,
        vertical: bool,
        bounds: [f64; 4],
    ) -> Option<StyleWarp> {
        if !style.is_preset() {
            return None;
        }
        let [x0, y0, x1, y1] = bounds;
        let (hw, hh) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
        if !(hw > 0.0 && hh > 0.0) {
            return None;
        }
        let pct = |v: f64| {
            if v.is_finite() {
                (v / 100.0).clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        let (b, hd, vd) = (pct(bend), pct(h_distort), pct(v_distort));
        if b == 0.0 && hd == 0.0 && vd == 0.0 {
            return None;
        }
        let grid = preset_grid(style, b, hd, vd, vertical, hh / hw);
        Some(StyleWarp {
            grid,
            o: (x0, y0),
            size: (2.0 * hw, 2.0 * hh),
        })
    }

    /// Longest segment (px) worth sending through [`StyleWarp::apply`] unsplit.
    pub fn max_segment(&self) -> f64 {
        (self.size.0.min(self.size.1) / 32.0).max(0.5)
    }

    /// Maps a point.
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let (u, v) = ((x - self.o.0) / self.size.0, (y - self.o.1) / self.size.1);
        let (bu, bv) = (bernstein(u), bernstein(v));
        let (mut px, mut py) = (0.0, 0.0);
        for (row, wv) in self.grid.iter().zip(bv) {
            for (p, wu) in row.iter().zip(bu) {
                px += wu * wv * p[0];
                py += wu * wv * p[1];
            }
        }
        (self.o.0 + px * self.size.0, self.o.1 + py * self.size.1)
    }

    /// The style as one Bezier patch over its box (every preset is one exactly).
    pub fn mesh(&self) -> BezierMesh {
        let points = self
            .grid
            .iter()
            .flatten()
            .map(|p| [self.o.0 + p[0] * self.size.0, self.o.1 + p[1] * self.size.1])
            .collect();
        BezierMesh {
            us: vec![0.0, 1.0],
            vs: vec![0.0, 1.0],
            points,
        }
    }
}

/// The rest grid (no warp).
fn rest_grid() -> Grid {
    std::array::from_fn(|j| std::array::from_fn(|i| [i as f64 / 3.0, j as f64 / 3.0]))
}

/// A horizontal edge at height `y` (box units, 0..1 across) bent into a circular arc of `|b|·π`
/// that keeps its chord, bowing up when `up == (b > 0)`. `h` is the box's height over its width:
/// the arc is circular in pixels.
fn edge_arc(y: f64, b: f64, up: bool, h: f64) -> [[f64; 2]; 4] {
    if b.abs() < 1e-9 {
        return std::array::from_fn(|i| [i as f64 / 3.0, y]);
    }
    let th = b.abs() * PI;
    let k = 4.0 / 3.0 * (th / 4.0).tan() * 0.5 / (th / 2.0).sin();
    let s = if (b > 0.0) == up { -1.0 } else { 1.0 };
    let (c, n) = ((th / 2.0).cos(), (th / 2.0).sin());
    let dy = s * k * n / h;
    [[0.0, y], [k * c, y + dy], [1.0 - k * c, y + dy], [1.0, y]]
}

/// Arc: every row is a circular arc of `|b|·π` around one centre; the edge on the inside of the
/// bend keeps its chord, and the rows' radii grow by the box height towards the other edge.
/// Worked in units of the box width (`h` = height / width) so the arcs are circular in pixels.
fn arc_grid(b: f64, h: f64) -> Grid {
    if b.abs() < 1e-9 {
        return rest_grid();
    }
    let up = b > 0.0;
    let th = b.abs() * PI;
    let r = 0.5 / (th / 2.0).sin();
    let k = 4.0 / 3.0 * (th / 4.0).tan();
    let s = if up { -1.0 } else { 1.0 };
    let cy = if up {
        h + r * (th / 2.0).cos()
    } else {
        -r * (th / 2.0).cos()
    };
    std::array::from_fn(|j| {
        let v = j as f64 / 3.0;
        let big = r + h * if up { 1.0 - v } else { v };
        let at = |a: f64| [0.5 + big * a.sin(), cy + s * big * a.cos()];
        let tangent = |a: f64| [a.cos(), -s * a.sin()];
        let (p0, p3, t0, t1) = (
            at(-th / 2.0),
            at(th / 2.0),
            tangent(-th / 2.0),
            tangent(th / 2.0),
        );
        let l = k * big;
        let pts = [
            p0,
            [p0[0] + l * t0[0], p0[1] + l * t0[1]],
            [p3[0] - l * t1[0], p3[1] - l * t1[1]],
            p3,
        ];
        pts.map(|p| [p[0], p[1] / h])
    })
}

/// Rows blended from `top` to `bottom`.
fn blend_rows(top: [[f64; 2]; 4], bottom: [[f64; 2]; 4]) -> Grid {
    std::array::from_fn(|j| {
        let t = j as f64 / 3.0;
        std::array::from_fn(|i| {
            [
                top[i][0] + (bottom[i][0] - top[i][0]) * t,
                top[i][1] + (bottom[i][1] - top[i][1]) * t,
            ]
        })
    })
}

/// Photoshop's warp presets as Bezier control grids, measured on Photoshop 27.11: every preset at
/// bend ±50 and ±100, Arc and Bulge with ±50 horizontal and vertical distortion, and the vertical
/// orientation, each fitted from scripted renders of a coordinate-coded grid image (every preset
/// is a single bicubic patch, within 0.5 px). These grids put Photoshop's pixels back within
/// 1 px (median) on all 70 cases, and on 32 more on 256 × 96 and 96 × 256 boxes, vertical, and
/// with both distortions: the arc family stays circular in pixels, the other offsets scale with
/// the box, and a vertical warp is the horizontal one with the axes (and distortions) swapped.
/// `b`, `hd`, `vd` are -1..1; `aspect` is the box's height over its width.
pub(super) fn preset_grid(
    style: WarpStyle,
    b: f64,
    hd: f64,
    vd: f64,
    vertical: bool,
    aspect: f64,
) -> Grid {
    // Height over width of the box the bend runs along (the transposed box when vertical); in
    // that frame the two distortions trade places too.
    let h = if vertical { 1.0 / aspect } else { aspect };
    let (hd, vd) = if vertical { (vd, hd) } else { (hd, vd) };
    let mut g = rest_grid();
    match style {
        WarpStyle::Arc => g = arc_grid(b, h),
        WarpStyle::Arch => g = std::array::from_fn(|j| edge_arc(j as f64 / 3.0, b, true, h)),
        WarpStyle::Bulge => g = blend_rows(edge_arc(0.0, b, true, h), edge_arc(1.0, b, false, h)),
        WarpStyle::ArcLower => g = blend_rows(g[0], edge_arc(1.0, b, false, h)),
        WarpStyle::ArcUpper => g = blend_rows(edge_arc(0.0, b, true, h), g[3]),
        // The bulging edge and the outer points of the row next to it come from Arc (bent the
        // other way for Shell Lower); the rest stays put.
        WarpStyle::ShellLower => {
            let a = arc_grid(-b, h);
            g[3] = a[3];
            g[2][0] = a[2][0];
            g[2][3] = a[2][3];
        }
        WarpStyle::ShellUpper => {
            let a = arc_grid(b, h);
            g[0] = a[0];
            g[1][0] = a[1][0];
            g[1][3] = a[1][3];
        }
        WarpStyle::Flag => {
            for row in &mut g {
                row[1][1] -= 2.0 * b;
                row[2][1] += 2.0 * b;
            }
        }
        WarpStyle::Wave => {
            for row in &mut g[1..3] {
                row[1][1] += 4.0 / 3.0 * b;
                row[2][1] -= 4.0 / 3.0 * b;
            }
        }
        WarpStyle::Fish => {
            for (row, f) in g.iter_mut().zip([-2.0, -2.0 / 3.0, 2.0 / 3.0, 2.0]) {
                row[1][1] += f * b;
                row[2][1] -= f * b;
            }
        }
        WarpStyle::Rise => {
            for row in &mut g {
                row[0][1] += 2.0 * b;
                row[1][1] += 2.0 * b;
            }
        }
        WarpStyle::Fisheye => {
            for (j, i, dx, dy) in [
                (1, 1, -1.0, -1.0),
                (1, 2, 1.0, -1.0),
                (2, 1, -1.0, 1.0),
                (2, 2, 1.0, 1.0),
            ] {
                g[j][i][0] += dx * 2.0 / 3.0 * b;
                g[j][i][1] += dy * 2.0 / 3.0 * b;
            }
        }
        // Edge midpoints move out (Inflate) or the sides in (Squeeze) by b/3, the inner points by b/9.
        WarpStyle::Inflate | WarpStyle::Squeeze => {
            let s = if style == WarpStyle::Inflate {
                1.0
            } else {
                -1.0
            };
            for p in &mut g[0][1..3] {
                p[1] -= b / 3.0;
            }
            for p in &mut g[3][1..3] {
                p[1] += b / 3.0;
            }
            for row in &mut g[1..3] {
                row[0][0] -= s * b / 3.0;
                row[3][0] += s * b / 3.0;
            }
            for (j, i, dx, dy) in [
                (1, 1, -1.0, -1.0),
                (1, 2, 1.0, -1.0),
                (2, 1, -1.0, 1.0),
                (2, 2, 1.0, 1.0),
            ] {
                g[j][i][0] += s * dx * b / 9.0;
                g[j][i][1] += dy * b / 9.0;
            }
        }
        // The inner points turn round the centre (the other way for a negative bend).
        WarpStyle::Twist => {
            let a = b.abs();
            if b >= 0.0 {
                g[1][1][0] += a;
                g[1][2][1] += a;
                g[2][1][1] -= a;
                g[2][2][0] -= a;
            } else {
                g[1][1][1] += a;
                g[1][2][0] -= a;
                g[2][1][0] += a;
                g[2][2][1] -= a;
            }
        }
        WarpStyle::None | WarpStyle::Custom => {}
    }
    // Vertical distortion: the top and bottom rows scale about their end points' midpoint (by
    // 1 ∓ vd) and the middle rows follow by the blend of those two moves.
    if vd != 0.0 {
        let moved = |row: [[f64; 2]; 4], s: f64| -> [[f64; 2]; 4] {
            let m = [(row[0][0] + row[3][0]) / 2.0, (row[0][1] + row[3][1]) / 2.0];
            std::array::from_fn(|i| {
                [
                    m[0] + (row[i][0] - m[0]) * s - row[i][0],
                    m[1] + (row[i][1] - m[1]) * s - row[i][1],
                ]
            })
        };
        let (d0, d3) = (moved(g[0], 1.0 - vd), moved(g[3], 1.0 + vd));
        for (j, row) in g.iter_mut().enumerate() {
            let t = j as f64 / 3.0;
            for (i, p) in row.iter_mut().enumerate() {
                p[0] += d0[i][0] + (d3[i][0] - d0[i][0]) * t;
                p[1] += d0[i][1] + (d3[i][1] - d0[i][1]) * t;
            }
        }
    }
    // Horizontal distortion: each column scales about its end points' midpoint by 1 + hd·(2i/3 − 1).
    if hd != 0.0 {
        for i in 0..4 {
            let s = 1.0 + hd * (2.0 * i as f64 / 3.0 - 1.0);
            let m = [
                (g[0][i][0] + g[3][i][0]) / 2.0,
                (g[0][i][1] + g[3][i][1]) / 2.0,
            ];
            for row in &mut g {
                row[i] = [m[0] + (row[i][0] - m[0]) * s, m[1] + (row[i][1] - m[1]) * s];
            }
        }
    }
    // Vertical orientation: the same grid with the axes swapped.
    if vertical {
        g = std::array::from_fn(|j| std::array::from_fn(|i| [g[i][j][1], g[i][j][0]]));
    }
    g
}
