//! The working window and its pixel and colour helpers.

use crate::fxutil::gauss_box_radii;
use rust_motion_geom::Rect;

pub(super) type Px = [f32; 3];

/// The working window: sRGB colour of every pixel of `r`.
pub(crate) struct Win {
    pub(super) r: Rect,
    pub(super) w: usize,
    pub(super) h: usize,
    pub(super) c: Vec<Px>,
}

impl Win {
    #[inline]
    pub(super) fn xy(&self, i: usize) -> (i32, i32) {
        (
            self.r.x0 + (i % self.w) as i32,
            self.r.y0 + (i / self.w) as i32,
        )
    }
    pub(super) fn lum(&self) -> Vec<f32> {
        self.c.iter().map(lum).collect()
    }
    /// Bilinear sample at document coordinates (pixel centres at .5), clamped to the window.
    pub(super) fn sample(&self, x: f32, y: f32) -> Px {
        let fx = (x - 0.5 - self.r.x0 as f32).clamp(0.0, (self.w - 1) as f32);
        let fy = (y - 0.5 - self.r.y0 as f32).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let g = |xx: usize, yy: usize| self.c[yy * self.w + xx];
        let top = mix(g(x0, y0), g(x1, y0), ax);
        let bot = mix(g(x0, y1), g(x1, y1), ax);
        mix(top, bot, ay)
    }
    /// The pixel nearest to document `(x, y)`, clamped to the window.
    pub(super) fn at(&self, x: i32, y: i32) -> Px {
        let xx = (x - self.r.x0).clamp(0, self.w as i32 - 1) as usize;
        let yy = (y - self.r.y0).clamp(0, self.h as i32 - 1) as usize;
        self.c[yy * self.w + xx]
    }
}

/// Hermite step from `e0` to `e1`; `e0 > e1` gives the falling step.
#[inline]
pub(super) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if (e1 - e0).abs() < 1e-9 {
        return if x >= e1 { 1.0 } else { 0.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub(super) fn lum(p: &Px) -> f32 {
    0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]
}

#[inline]
pub(super) fn mix(a: Px, b: Px, t: f32) -> Px {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[inline]
pub(super) fn map(p: Px, f: impl Fn(f32) -> f32) -> Px {
    [f(p[0]), f(p[1]), f(p[2])]
}

#[inline]
pub(super) fn grey(v: f32) -> Px {
    [v, v, v]
}

#[inline]
pub(super) fn rgb(c: [f32; 4]) -> Px {
    [c[0], c[1], c[2]]
}

#[inline]
pub(super) fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

#[inline]
pub(super) fn posterize(v: f32, levels: f32) -> f32 {
    let l = (levels - 1.0).max(1.0);
    (clamp01(v) * l).round() / l
}

#[inline]
pub(super) fn saturate(p: Px, k: f32) -> Px {
    let l = lum(&p);
    map(p, |v| l + (v - l) * k)
}

/// Pixels a Gaussian blur of `sigma` reads (three box passes per axis).
pub(super) fn gs(sigma: f32) -> i32 {
    if sigma < 0.3 {
        0
    } else {
        gauss_box_radii(sigma).iter().sum::<usize>() as i32 + 1
    }
}
