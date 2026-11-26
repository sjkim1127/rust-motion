//! Shared drawing types: vector primitives, shapes and blend modes.

use super::*;

// ---------------------------------------------------------------------------
// Primitives and compositing
// ---------------------------------------------------------------------------

/// Geometry of a primitive (document pixel coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A segment from `a` to `b` whose radius goes from `ra` to `rb`; colour goes `c0` → `c1` along it.
    Capsule {
        a: (f32, f32),
        b: (f32, f32),
        ra: f32,
        rb: f32,
    },
    /// An ellipse rotated by `angle` radians; colour goes `c0` (centre) → `c1` (rim).
    Ellipse {
        c: (f32, f32),
        rx: f32,
        ry: f32,
        angle: f32,
    },
}

/// One drawable primitive. Colours are straight sRGB RGBA (0–1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prim {
    pub shape: Shape,
    pub c0: [f32; 4],
    pub c1: [f32; 4],
    /// Edge softness in pixels (≥ 1 for anti-aliasing; larger for glows).
    pub soft: f32,
    pub blend: Blend,
}

/// How a primitive combines with what is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Blend {
    /// Source over.
    #[default]
    Normal,
    /// Additive light (glows).
    Add,
    /// Per-channel maximum of the premultiplied values (overlapping strands don't build up).
    Lighten,
}

impl Prim {
    pub fn capsule(
        a: (f32, f32),
        b: (f32, f32),
        ra: f32,
        rb: f32,
        c0: [f32; 4],
        c1: [f32; 4],
    ) -> Prim {
        Prim {
            shape: Shape::Capsule { a, b, ra, rb },
            c0,
            c1,
            soft: 1.0,
            blend: Blend::Normal,
        }
    }
    pub fn ellipse(
        c: (f32, f32),
        rx: f32,
        ry: f32,
        angle: f32,
        c0: [f32; 4],
        c1: [f32; 4],
    ) -> Prim {
        Prim {
            shape: Shape::Ellipse { c, rx, ry, angle },
            c0,
            c1,
            soft: 1.0,
            blend: Blend::Normal,
        }
    }
    pub fn disc(c: (f32, f32), r: f32, col: [f32; 4]) -> Prim {
        Prim::ellipse(c, r, r, 0.0, col, col)
    }
    pub(super) fn with_soft(mut self, s: f32) -> Prim {
        self.soft = s;
        self
    }
    pub(super) fn additive(mut self) -> Prim {
        self.blend = Blend::Add;
        self
    }
    pub(super) fn lighten(mut self) -> Prim {
        self.blend = Blend::Lighten;
        self
    }

    /// Pixel bounds this primitive can touch.
    pub fn bounds(&self) -> Rect {
        let s = self.soft.max(1.0) + 1.0;
        let (x0, y0, x1, y1) = match self.shape {
            Shape::Capsule { a, b, ra, rb } => {
                let r = ra.max(rb) + s;
                (
                    a.0.min(b.0) - r,
                    a.1.min(b.1) - r,
                    a.0.max(b.0) + r,
                    a.1.max(b.1) + r,
                )
            }
            Shape::Ellipse { c, rx, ry, angle } => {
                let (sn, cs) = angle.sin_cos();
                let ex = ((rx * cs).powi(2) + (ry * sn).powi(2)).sqrt() + s;
                let ey = ((rx * sn).powi(2) + (ry * cs).powi(2)).sqrt() + s;
                (c.0 - ex, c.1 - ey, c.0 + ex, c.1 + ey)
            }
        };
        if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return Rect::EMPTY;
        }
        let cl = |v: f32| v.clamp(-1.0e8, 1.0e8) as i32;
        Rect::new(cl(x0.floor()), cl(y0.floor()), cl(x1.ceil()), cl(y1.ceil()))
    }

    /// Coverage (0–1) and colour position (0–1) at a pixel centre.
    #[inline]
    pub(super) fn eval(&self, x: f32, y: f32) -> (f32, f32) {
        let soft = self.soft.max(1.0);
        match self.shape {
            Shape::Capsule { a, b, ra, rb } => {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let l2 = dx * dx + dy * dy;
                let t = if l2 > 1e-9 {
                    (((x - a.0) * dx + (y - a.1) * dy) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (px, py) = (a.0 + dx * t - x, a.1 + dy * t - y);
                let d = (px * px + py * py).sqrt();
                let r = ra + (rb - ra) * t;
                // Hairlines thinner than a pixel fade instead of vanishing.
                let thin = (2.0 * r).clamp(0.0, 1.0);
                (((r.max(0.5) - d) / soft + 0.5).clamp(0.0, 1.0) * thin, t)
            }
            Shape::Ellipse { c, rx, ry, angle } => {
                let (s, co) = angle.sin_cos();
                let (dx, dy) = (x - c.0, y - c.1);
                let (u, v) = (dx * co + dy * s, -dx * s + dy * co);
                let (rx, ry) = (rx.max(0.05), ry.max(0.05));
                let e = ((u / rx).powi(2) + (v / ry).powi(2)).sqrt();
                let edge = (1.0 - e) * rx.min(ry);
                let thin = (2.0 * rx.min(ry)).clamp(0.0, 1.0);
                ((edge / soft + 0.5).clamp(0.0, 1.0) * thin, e.min(1.0))
            }
        }
    }
}
