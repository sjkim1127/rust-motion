//! Filter › Render › Picture Frame: ornaments walked along the frame edge.

use super::*;

// ---------------------------------------------------------------------------
// Picture Frame
// ---------------------------------------------------------------------------

/// Picture frame designs (index = `frame` − 1).
pub const FRAME_STYLES: [&str; 16] = [
    "vineWithFlowers",
    "vineWithLeaves",
    "ivy",
    "roses",
    "daisies",
    "berries",
    "bamboo",
    "waves",
    "zigzag",
    "dots",
    "rope",
    "scallops",
    "doubleLine",
    "snowflakes",
    "stars",
    "hearts",
];

/// Picture Frame options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FrameSpec {
    /// 1-based design (see [`FRAME_STYLES`]).
    pub frame: u32,
    /// Distance of the frame from the canvas edge, % of the shorter side (0–30).
    pub margin: f32,
    /// Ornament size 1–100.
    pub size: f32,
    /// Ornament spacing / density 1–100 (higher = denser).
    pub arrangement: f32,
    /// Number of parallel lines for line designs (1–5).
    pub lines: u32,
    /// Line / vine thickness 1–100.
    pub thickness: f32,
    /// 0–100 %: fade the ornaments towards the inside.
    pub fade: f32,
    pub vine_color: [f32; 4],
    pub flower_color: [f32; 4],
    pub leaf_color: [f32; 4],
    pub seed: u32,
}

impl Default for FrameSpec {
    fn default() -> Self {
        FrameSpec {
            frame: 1,
            margin: 4.0,
            size: 50.0,
            arrangement: 50.0,
            lines: 1,
            thickness: 30.0,
            fade: 0.0,
            vine_color: [0.25, 0.4, 0.15, 1.0],
            flower_color: [0.9, 0.3, 0.45, 1.0],
            leaf_color: [0.3, 0.6, 0.2, 1.0],
            seed: 0,
        }
    }
}

/// The frame rectangle walked by arc length (clockwise from the top-left).
struct Border {
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
}

impl Border {
    fn len(&self) -> f32 {
        2.0 * (self.w + self.h)
    }
    /// Point, unit tangent and inward normal at arc length `s`.
    fn at(&self, s: f32) -> ((f32, f32), (f32, f32), (f32, f32)) {
        let s = s.rem_euclid(self.len());
        let (w, h) = (self.w, self.h);
        if s < w {
            ((self.x0 + s, self.y0), (1.0, 0.0), (0.0, 1.0))
        } else if s < w + h {
            ((self.x0 + w, self.y0 + s - w), (0.0, 1.0), (-1.0, 0.0))
        } else if s < 2.0 * w + h {
            (
                (self.x0 + w - (s - w - h), self.y0 + h),
                (-1.0, 0.0),
                (0.0, -1.0),
            )
        } else {
            (
                (self.x0, self.y0 + h - (s - 2.0 * w - h)),
                (0.0, -1.0),
                (1.0, 0.0),
            )
        }
    }
}

fn flower(
    out: &mut Vec<Prim>,
    c: (f32, f32),
    r: f32,
    petals: usize,
    petal: [f32; 4],
    centre: [f32; 4],
    rot: f32,
) {
    for k in 0..petals {
        let a = rot + k as f32 / petals as f32 * std::f32::consts::TAU;
        let p = (c.0 + a.cos() * r * 0.55, c.1 + a.sin() * r * 0.55);
        out.push(Prim::ellipse(
            p,
            r * 0.5,
            r * 0.26,
            a,
            petal,
            shade([petal[0], petal[1], petal[2]], 0.8),
        ));
    }
    out.push(Prim::ellipse(
        c,
        r * 0.28,
        r * 0.28,
        0.0,
        centre,
        shade([centre[0], centre[1], centre[2]], 0.75),
    ));
}

pub(super) fn leaf(out: &mut Vec<Prim>, p: (f32, f32), ang: f32, len: f32, col: [f32; 4]) {
    let c = (p.0 + ang.cos() * len * 0.5, p.1 + ang.sin() * len * 0.5);
    out.push(Prim::ellipse(
        c,
        len * 0.5,
        len * 0.22,
        ang,
        shade([col[0], col[1], col[2]], 1.12),
        shade([col[0], col[1], col[2]], 0.8),
    ));
}

fn star(out: &mut Vec<Prim>, c: (f32, f32), r: f32, points: usize, col: [f32; 4], rot: f32) {
    for k in 0..points {
        let a = rot + k as f32 / points as f32 * std::f32::consts::TAU;
        out.push(Prim::capsule(
            c,
            (c.0 + a.cos() * r, c.1 + a.sin() * r),
            r * 0.28,
            0.4,
            col,
            col,
        ));
    }
}

/// Builds picture-frame primitives inside `canvas`.
pub fn picture_frame(spec: &FrameSpec, canvas: Rect) -> Vec<Prim> {
    let mut out = Vec::new();
    let short = (canvas.width().min(canvas.height()) as f32).max(8.0);
    let margin = short * spec.margin.clamp(0.0, 30.0) / 100.0;
    let unit = short * (0.008 + spec.size.clamp(1.0, 100.0) / 100.0 * 0.05);
    let thick = (unit * 0.04 + short * 0.00012 * spec.thickness.clamp(1.0, 100.0)).max(0.6);
    let spacing = unit * (4.0 - 3.2 * spec.arrangement.clamp(1.0, 100.0) / 100.0);
    let inset = margin + unit;
    let b = Border {
        x0: canvas.x0 as f32 + inset,
        y0: canvas.y0 as f32 + inset,
        w: (canvas.width() as f32 - 2.0 * inset).max(1.0),
        h: (canvas.height() as f32 - 2.0 * inset).max(1.0),
    };
    let total = b.len();
    let count = (total / spacing).round().max(4.0) as usize;
    let step = total / count as f32;
    let mut rng = Rng::new(spec.seed, 0xF8A3E + spec.frame as u64);
    let (vine, fl, lf) = (spec.vine_color, spec.flower_color, spec.leaf_color);
    let style = FRAME_STYLES[(spec.frame.clamp(1, FRAME_STYLES.len() as u32) - 1) as usize];
    let lines = spec.lines.clamp(1, 5) as usize;
    let white = [1.0, 1.0, 1.0, 1.0];
    let yellow = [0.98, 0.82, 0.2, 1.0];
    // A wavy vine along the border.
    let vine_path = |out: &mut Vec<Prim>, amp: f32, wl: f32, col: [f32; 4], r: f32| {
        let n = (total / 3.0).ceil().max(8.0) as usize;
        let mut prev: Option<(f32, f32)> = None;
        for k in 0..=n {
            let s = k as f32 / n as f32 * total;
            let (p, _, nn) = b.at(s);
            let o = amp * (s / wl * std::f32::consts::TAU).sin();
            let q = (p.0 + nn.0 * o, p.1 + nn.1 * o);
            if let Some(pp) = prev {
                out.push(Prim::capsule(pp, q, r, r, col, col));
            }
            prev = Some(q);
        }
    };
    let wave_off = |s: f32, amp: f32, wl: f32| amp * (s / wl * std::f32::consts::TAU).sin();
    match style {
        "vineWithFlowers" | "vineWithLeaves" | "ivy" | "roses" | "daisies" | "berries" => {
            let amp = unit * 0.5;
            let wl = step * 2.0;
            vine_path(&mut out, amp, wl, vine, thick);
            for k in 0..count {
                let s = k as f32 * step + step * 0.5;
                let (p, t, nn) = b.at(s);
                let o = wave_off(s, amp, wl);
                let q = (p.0 + nn.0 * o, p.1 + nn.1 * o);
                let side = if k % 2 == 0 { 1.0 } else { -1.0 };
                let ta = t.1.atan2(t.0);
                let la = ta + side * 0.9 + rng.sym() * 0.3;
                let ll = unit * rng.range(0.9, 1.2);
                match style {
                    "vineWithFlowers" => {
                        leaf(&mut out, q, la, ll, lf);
                        if k % 2 == 0 {
                            let c = (
                                q.0 + nn.0 * unit * 0.7 * side,
                                q.1 + nn.1 * unit * 0.7 * side,
                            );
                            flower(&mut out, c, unit * 0.65, 5, fl, yellow, rng.f() * 6.3);
                        }
                    }
                    "vineWithLeaves" => {
                        leaf(&mut out, q, la, ll, lf);
                        leaf(&mut out, q, ta - side * 0.9, ll * 0.8, lf);
                    }
                    "ivy" => {
                        for j in 0..3 {
                            let a = la + (j as f32 - 1.0) * 0.7;
                            leaf(
                                &mut out,
                                q,
                                a,
                                ll * 0.75,
                                shade([lf[0], lf[1], lf[2]], 0.75 + 0.15 * j as f32),
                            );
                        }
                    }
                    "roses" => {
                        leaf(&mut out, q, la, ll, lf);
                        if k % 3 == 0 {
                            let r = unit * 0.7;
                            for (j, f) in [1.0f32, 0.75, 0.5, 0.28].iter().enumerate() {
                                let c = shade([fl[0], fl[1], fl[2]], 0.75 + 0.1 * j as f32);
                                out.push(Prim::ellipse(
                                    q,
                                    r * f,
                                    r * f * 0.9,
                                    rng.f() * 3.0,
                                    c,
                                    shade([fl[0], fl[1], fl[2]], 0.55),
                                ));
                            }
                        }
                    }
                    "daisies" => {
                        leaf(&mut out, q, la, ll * 0.8, lf);
                        if k % 2 == 1 {
                            flower(&mut out, q, unit * 0.6, 12, white, yellow, rng.f());
                        }
                    }
                    _ => {
                        leaf(&mut out, q, la, ll, lf);
                        for _ in 0..3 {
                            let c = (q.0 + rng.sym() * unit * 0.4, q.1 + rng.sym() * unit * 0.4);
                            out.push(Prim::ellipse(
                                c,
                                unit * 0.2,
                                unit * 0.2,
                                0.0,
                                shade([fl[0], fl[1], fl[2]], 1.2),
                                shade([fl[0], fl[1], fl[2]], 0.6),
                            ));
                        }
                    }
                }
            }
        }
        "bamboo" => {
            for edge in 0..4 {
                let (s0, l) = match edge {
                    0 => (0.0, b.w),
                    1 => (b.w, b.h),
                    2 => (b.w + b.h, b.w),
                    _ => (2.0 * b.w + b.h, b.h),
                };
                let (a, _, _) = b.at(s0 + 0.01);
                let (z, _, _) = b.at(s0 + l - 0.01);
                let joints = (l / (spacing * 2.0)).round().max(1.0) as usize;
                for j in 0..joints {
                    let f0 = j as f32 / joints as f32;
                    let f1 = (j + 1) as f32 / joints as f32;
                    let p0 = (a.0 + (z.0 - a.0) * f0, a.1 + (z.1 - a.1) * f0);
                    let p1 = (a.0 + (z.0 - a.0) * f1, a.1 + (z.1 - a.1) * f1);
                    out.push(Prim::capsule(
                        p0,
                        p1,
                        unit * 0.35,
                        unit * 0.35,
                        vine,
                        shade([vine[0], vine[1], vine[2]], 0.8),
                    ));
                    out.push(Prim::disc(
                        p1,
                        unit * 0.4,
                        shade([vine[0], vine[1], vine[2]], 0.7),
                    ));
                }
                // A few leaves per edge.
                for _ in 0..(joints / 2).max(1) {
                    let f = rng.f();
                    let p = (a.0 + (z.0 - a.0) * f, a.1 + (z.1 - a.1) * f);
                    leaf(&mut out, p, rng.f() * 6.3, unit * 1.4, lf);
                }
            }
        }
        "waves" | "zigzag" | "doubleLine" | "rope" => {
            for li in 0..lines {
                let off = (li as f32 - (lines - 1) as f32 / 2.0) * thick * 4.0;
                let n = (total / 2.0).ceil().max(8.0) as usize;
                let mut prev: Option<(f32, f32)> = None;
                for k in 0..=n {
                    let s = k as f32 / n as f32 * total;
                    let (p, _, nn) = b.at(s);
                    let o = off
                        + match style {
                            "waves" => wave_off(s + li as f32 * step * 0.25, unit * 0.4, step),
                            "zigzag" => {
                                let ph = (s / step).fract();
                                unit * 0.4 * (4.0 * (ph - 0.5).abs() - 1.0)
                            }
                            _ => 0.0,
                        };
                    let q = (p.0 + nn.0 * o, p.1 + nn.1 * o);
                    if style == "rope" {
                        if k % 2 == 0 {
                            let (_, t, _) = b.at(s);
                            out.push(Prim::ellipse(
                                q,
                                unit * 0.45,
                                unit * 0.2,
                                t.1.atan2(t.0) + 0.7,
                                shade([vine[0], vine[1], vine[2]], 1.1),
                                shade([vine[0], vine[1], vine[2]], 0.65),
                            ));
                        }
                    } else if let Some(pp) = prev {
                        out.push(Prim::capsule(pp, q, thick, thick, vine, vine));
                    }
                    prev = Some(q);
                }
                if style == "doubleLine" {
                    // A second rule inside each line and corner squares.
                    let inner = Border {
                        x0: b.x0 + unit * 0.6 + off,
                        y0: b.y0 + unit * 0.6 + off,
                        w: b.w - 2.0 * (unit * 0.6 + off),
                        h: b.h - 2.0 * (unit * 0.6 + off),
                    };
                    for e in 0..4 {
                        let (p, _, _) =
                            inner.at([0.0, inner.w, inner.w + inner.h, 2.0 * inner.w + inner.h][e]);
                        let (q, _, _) = inner.at([
                            inner.w,
                            inner.w + inner.h,
                            2.0 * inner.w + inner.h,
                            inner.len() - 0.001,
                        ][e]);
                        out.push(Prim::capsule(p, q, thick * 0.6, thick * 0.6, vine, vine));
                    }
                }
            }
            if style == "doubleLine" {
                for e in 0..4 {
                    let (p, _, _) = b.at([0.0, b.w, b.w + b.h, 2.0 * b.w + b.h][e]);
                    out.push(Prim::ellipse(
                        p,
                        unit * 0.45,
                        unit * 0.45,
                        std::f32::consts::FRAC_PI_4,
                        fl,
                        fl,
                    ));
                }
            }
        }
        "dots" | "scallops" | "snowflakes" | "stars" | "hearts" => {
            for k in 0..count {
                let s = k as f32 * step;
                let (p, t, nn) = b.at(s);
                let ta = t.1.atan2(t.0);
                match style {
                    "dots" => {
                        let r = unit * if k % 2 == 0 { 0.4 } else { 0.22 };
                        out.push(Prim::ellipse(
                            p,
                            r,
                            r,
                            0.0,
                            shade([fl[0], fl[1], fl[2]], 1.15),
                            shade([fl[0], fl[1], fl[2]], 0.75),
                        ));
                    }
                    "scallops" => {
                        let (c0, _, _) = b.at(s - step * 0.5);
                        let mid = ((c0.0 + p.0) / 2.0, (c0.1 + p.1) / 2.0);
                        let n = 8;
                        for j in 0..n {
                            let a0 = std::f32::consts::PI * j as f32 / n as f32;
                            let a1 = std::f32::consts::PI * (j + 1) as f32 / n as f32;
                            let r = step * 0.5;
                            let pt = |a: f32| {
                                (
                                    mid.0 - t.0 * r * a.cos() - nn.0 * r * a.sin(),
                                    mid.1 - t.1 * r * a.cos() - nn.1 * r * a.sin(),
                                )
                            };
                            out.push(Prim::capsule(pt(a0), pt(a1), thick, thick, vine, vine));
                        }
                    }
                    "snowflakes" => {
                        let r = unit * 0.6;
                        let c = [0.85, 0.92, 1.0, 1.0];
                        for j in 0..6 {
                            let a = ta + j as f32 * std::f32::consts::FRAC_PI_3;
                            let e = (p.0 + a.cos() * r, p.1 + a.sin() * r);
                            out.push(Prim::capsule(
                                p,
                                e,
                                (thick * 0.6).max(0.6),
                                (thick * 0.4).max(0.5),
                                c,
                                c,
                            ));
                            let m = (p.0 + a.cos() * r * 0.6, p.1 + a.sin() * r * 0.6);
                            for sd in [-0.7f32, 0.7] {
                                let b2 = (
                                    m.0 + (a + sd).cos() * r * 0.3,
                                    m.1 + (a + sd).sin() * r * 0.3,
                                );
                                out.push(Prim::capsule(m, b2, (thick * 0.4).max(0.5), 0.4, c, c));
                            }
                        }
                    }
                    "stars" => star(
                        &mut out,
                        p,
                        unit * 0.55,
                        5,
                        if k % 2 == 0 { yellow } else { fl },
                        ta - std::f32::consts::FRAC_PI_2,
                    ),
                    _ => {
                        // Heart: two lobes and a point towards the inside.
                        let r = unit * 0.3;
                        let (tx, ty) = t;
                        let l = (
                            p.0 - tx * r * 0.8 - nn.0 * r * 0.3,
                            p.1 - ty * r * 0.8 - nn.1 * r * 0.3,
                        );
                        let rr = (
                            p.0 + tx * r * 0.8 - nn.0 * r * 0.3,
                            p.1 + ty * r * 0.8 - nn.1 * r * 0.3,
                        );
                        let tip = (p.0 + nn.0 * r * 1.6, p.1 + nn.1 * r * 1.6);
                        out.push(Prim::capsule(l, tip, r, 0.6, fl, fl));
                        out.push(Prim::capsule(rr, tip, r, 0.6, fl, fl));
                    }
                }
            }
        }
        _ => {}
    }
    // Fade: lower the opacity of ornaments that reach further inside.
    let fade = spec.fade.clamp(0.0, 100.0) / 100.0;
    if fade > 0.0 {
        let cx = canvas.x0 as f32 + canvas.width() as f32 / 2.0;
        let cy = canvas.y0 as f32 + canvas.height() as f32 / 2.0;
        for p in &mut out {
            let c = match p.shape {
                Shape::Capsule { a, .. } => a,
                Shape::Ellipse { c, .. } => c,
            };
            let edge = ((c.0 - cx).abs() / (b.w / 2.0))
                .max((c.1 - cy).abs() / (b.h / 2.0))
                .clamp(0.0, 1.0);
            let k = 1.0 - fade * (1.0 - edge) * 8.0;
            p.c0[3] *= k.clamp(0.0, 1.0);
            p.c1[3] *= k.clamp(0.0, 1.0);
        }
    }
    out
}
