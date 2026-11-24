//! [`Warp`]: a style or custom mesh over a source box, and the forward map it defines.

use super::*;

/// A complete warp: what Photoshop's `warp` descriptor stores, plus the custom mesh.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Warp {
    pub style: WarpStyle,
    /// Bend in percent (-100..100), presets only.
    #[serde(default)]
    pub bend: f64,
    #[serde(default)]
    pub h_distort: f64,
    #[serde(default)]
    pub v_distort: f64,
    /// Warp along the vertical axis.
    #[serde(default)]
    pub vertical: bool,
    /// The source box `[x0, y0, x1, y1]` the warp is defined over.
    pub bounds: [f64; 4],
    /// Control mesh for [`WarpStyle::Custom`] (output coordinates).
    #[serde(default)]
    pub mesh: Option<BezierMesh>,
}

impl Warp {
    /// No warp over `bounds`.
    pub fn none(bounds: [f64; 4]) -> Warp {
        Warp {
            style: WarpStyle::None,
            bend: 0.0,
            h_distort: 0.0,
            v_distort: 0.0,
            vertical: false,
            bounds,
            mesh: None,
        }
    }

    /// A preset style.
    pub fn preset(style: WarpStyle, bend: f64, bounds: [f64; 4]) -> Warp {
        Warp {
            style,
            bend,
            ..Warp::none(bounds)
        }
    }

    /// A custom warp from a mesh.
    pub fn custom(mesh: BezierMesh, bounds: [f64; 4]) -> Warp {
        Warp {
            style: WarpStyle::Custom,
            mesh: Some(mesh),
            ..Warp::none(bounds)
        }
    }

    fn style_warp(&self) -> Option<StyleWarp> {
        StyleWarp::new(
            self.style,
            self.bend,
            self.h_distort,
            self.v_distort,
            self.vertical,
            self.bounds,
        )
    }

    /// Whether the warp maps every point of its box onto itself (within `1e-9` px).
    pub fn is_identity(&self) -> bool {
        match self.style {
            WarpStyle::None => true,
            WarpStyle::Custom => match &self.mesh {
                None => true,
                Some(m) => {
                    let id =
                        BezierMesh::fit(&|s, t| self.box_point(s, t), m.us.clone(), m.vs.clone());
                    m.points
                        .iter()
                        .zip(&id.points)
                        .all(|(a, b)| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9)
                }
            },
            _ => self.style_warp().is_none(),
        }
    }

    fn box_point(&self, s: f64, t: f64) -> [f64; 2] {
        let b = self.bounds;
        [b[0] + s * (b[2] - b[0]), b[1] + t * (b[3] - b[1])]
    }

    /// Maps a source point (inside the box) to output space.
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        match self.style {
            WarpStyle::None => (x, y),
            WarpStyle::Custom => match &self.mesh {
                Some(m) => {
                    let b = self.bounds;
                    let (w, h) = (b[2] - b[0], b[3] - b[1]);
                    if w <= 0.0 || h <= 0.0 {
                        return (x, y);
                    }
                    let p = m.eval((x - b[0]) / w, (y - b[1]) / h);
                    (p[0], p[1])
                }
                None => (x, y),
            },
            _ => match self.style_warp() {
                Some(sw) => sw.apply(x, y),
                None => (x, y),
            },
        }
    }

    /// The warp as a control mesh (a preset is its own Bezier patch split into `cols × rows`;
    /// custom warps return their own mesh).
    pub fn to_mesh(&self, cols: usize, rows: usize) -> BezierMesh {
        if let (WarpStyle::Custom, Some(m)) = (self.style, &self.mesh) {
            return m.clone();
        }
        let (cols, rows) = (cols.clamp(1, 64), rows.clamp(1, 64));
        // A preset is one Bezier patch: split it exactly.
        if let Some(sw) = self.style_warp() {
            let mut m = sw.mesh();
            for i in 1..cols {
                m.split_u(i as f64 / cols as f64);
            }
            for j in 1..rows {
                m.split_v(j as f64 / rows as f64);
            }
            return m;
        }
        let us: Vec<f64> = (0..=cols).map(|i| i as f64 / cols as f64).collect();
        let vs: Vec<f64> = (0..=rows).map(|i| i as f64 / rows as f64).collect();
        BezierMesh::fit(
            &|s, t| {
                let p = self.box_point(s, t);
                let (x, y) = self.map(p[0], p[1]);
                [x, y]
            },
            us,
            vs,
        )
    }

    /// The same warp followed by the affine map `[a, b, c, d, e, f]` (`x' = a·x + c·y + e`,
    /// `y' = b·x + d·y + f`), as a custom mesh (exact for custom warps).
    pub fn then_affine(&self, m: [f64; 6]) -> Warp {
        let mesh = self.to_mesh(4, 4).map_points(|p| {
            [
                m[0] * p[0] + m[2] * p[1] + m[4],
                m[1] * p[0] + m[3] * p[1] + m[5],
            ]
        });
        Warp::custom(mesh, self.bounds)
    }

    /// Approximate output bounds (sampled; includes the control mesh for custom warps).
    pub fn output_bounds(&self) -> [f64; 4] {
        let n = 32;
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for j in 0..=n {
            for i in 0..=n {
                let p = self.box_point(i as f64 / n as f64, j as f64 / n as f64);
                let (x, y) = self.map(p[0], p[1]);
                b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
        }
        b
    }
}
