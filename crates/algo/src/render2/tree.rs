//! Filter › Render › Tree: recursive branching and foliage.

use super::*;

/// Tree options (Photoshop's Tree dialog).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TreeSpec {
    /// 1–34.
    pub base_tree_type: u32,
    /// 1 (light from the left) … 3 (above) … 5 (right).
    pub light_direction: u32,
    /// 0–100.
    pub leaves_amount: f32,
    /// 0–200 %.
    pub leaves_size: f32,
    /// 50–300 %: how high the branching starts.
    pub branches_height: f32,
    /// 50–200 %.
    pub branches_thickness: f32,
    /// Use the type's leaf colour; otherwise `leaves_color`.
    pub default_leaves: bool,
    pub leaves_color: [f32; 4],
    /// Use `branches_color` instead of the type's bark.
    pub custom_branch_color: bool,
    pub branches_color: [f32; 4],
    /// Flat shading (no light variation).
    pub flat_shading: bool,
    pub seed: u32,
    /// Base position as a fraction of the canvas.
    pub x: f32,
    pub y: f32,
    /// Height as a fraction of the canvas height.
    pub size: f32,
}

impl Default for TreeSpec {
    fn default() -> Self {
        TreeSpec {
            base_tree_type: 1,
            light_direction: 3,
            leaves_amount: 50.0,
            leaves_size: 100.0,
            branches_height: 100.0,
            branches_thickness: 100.0,
            default_leaves: true,
            leaves_color: [0.24, 0.52, 0.16, 1.0],
            custom_branch_color: false,
            branches_color: [0.36, 0.25, 0.16, 1.0],
            flat_shading: false,
            seed: 0,
            x: 0.5,
            y: 0.95,
            size: 0.8,
        }
    }
}

struct TreeCtx<'a> {
    kind: &'a TreeKind,
    spec: &'a TreeSpec,
    rng: Rng,
    branches: Vec<Prim>,
    leaves: Vec<(f32, Prim)>,
    bark: [f32; 3],
    leaf: [f32; 3],
    light: (f32, f32),
    centre: (f32, f32),
    leaf_px: f32,
}

pub(super) fn shade(c: [f32; 3], k: f32) -> [f32; 4] {
    [
        (c[0] * k).clamp(0.0, 1.0),
        (c[1] * k).clamp(0.0, 1.0),
        (c[2] * k).clamp(0.0, 1.0),
        1.0,
    ]
}

impl TreeCtx<'_> {
    /// Brightness of a leaf at `p` (lit side of the crown brighter).
    fn light_at(&mut self, p: (f32, f32)) -> f32 {
        if self.spec.flat_shading {
            return 1.0;
        }
        let (dx, dy) = (p.0 - self.centre.0, p.1 - self.centre.1);
        let l = (dx * dx + dy * dy).sqrt().max(1.0);
        0.78 + 0.32 * ((dx * self.light.0 + dy * self.light.1) / l) + 0.12 * self.rng.sym()
    }

    fn add_leaf(&mut self, p: (f32, f32), dir: f32) {
        let r = &mut self.rng;
        let s = self.leaf_px * r.range(0.7, 1.2);
        let ang = dir + r.sym() * 1.2;
        let k = self.light_at(p);
        let col = self.leaf;
        let prim = match self.kind.leaf {
            Leaf::None => return,
            Leaf::Round => Prim::ellipse(
                p,
                s,
                s * 0.85,
                ang,
                shade(col, k * 1.1),
                shade(col, k * 0.8),
            ),
            Leaf::Oval => Prim::ellipse(
                p,
                s * 1.2,
                s * 0.55,
                ang,
                shade(col, k * 1.1),
                shade(col, k * 0.8),
            ),
            Leaf::Long => Prim::ellipse(
                p,
                s * 1.6,
                s * 0.28,
                ang,
                shade(col, k * 1.1),
                shade(col, k * 0.8),
            ),
            Leaf::Heart => Prim::ellipse(
                p,
                s * 0.9,
                s * 1.1,
                ang,
                shade(col, k * 1.1),
                shade(col, k * 0.75),
            ),
            Leaf::Needle => {
                // A tuft of needles fanning out from the twig.
                let n = 7;
                for j in 0..n {
                    let a = ang + (j as f32 / (n - 1) as f32 - 0.5) * 2.4 + self.rng.sym() * 0.15;
                    let l = s * self.rng.range(1.1, 1.7);
                    let q = (p.0 + a.cos() * l, p.1 + a.sin() * l);
                    let kk = k + 0.1 * self.rng.sym();
                    self.leaves.push((
                        kk,
                        Prim::capsule(
                            p,
                            q,
                            (s * 0.1).max(0.45),
                            (s * 0.05).max(0.3),
                            shade(col, kk * 0.85),
                            shade(col, kk * 1.1),
                        ),
                    ));
                }
                return;
            }
            Leaf::Blossom => Prim::ellipse(
                p,
                s * 0.75,
                s * 0.75,
                0.0,
                shade(col, k * 1.08),
                shade(col, k * 0.85),
            ),
            Leaf::Frond => {
                Prim::ellipse(p, s * 1.6, s * 0.3, ang, shade(col, k), shade(col, k * 0.8))
            }
        };
        let order = k + self.rng.f() * 0.05;
        self.leaves.push((order, prim));
        if let Some(acc) = self.kind.accent {
            if self.rng.f() < 0.12 {
                let q = (p.0 + self.rng.sym() * s, p.1 + self.rng.sym() * s);
                let r = (s * 0.45).max(1.0);
                self.leaves.push((
                    k + 1.0,
                    Prim::ellipse(q, r, r, 0.0, shade(acc, k * 1.15), shade(acc, k * 0.75)),
                ));
            }
        }
    }

    /// A curved branch from `p` heading `ang` (radians, 0 = right, y down).
    /// Returns its end point and end heading.
    fn limb(
        &mut self,
        p: (f32, f32),
        ang: f32,
        len: f32,
        r0: f32,
        r1: f32,
        depth01: f32,
    ) -> ((f32, f32), f32) {
        let segs = ((len / 6.0) as usize).clamp(2, 10);
        let (mut q, mut a) = (p, ang);
        let droop = self.kind.droop * depth01;
        let col0 = shade(self.bark, 0.9 + 0.2 * depth01);
        for k in 0..segs {
            let t0 = k as f32 / segs as f32;
            let t1 = (k + 1) as f32 / segs as f32;
            a += self.kind.wiggle * self.rng.sym() * 0.25;
            // Gravity pulls the heading towards straight down (π/2).
            let down = std::f32::consts::FRAC_PI_2;
            let diff = (down - a).sin().atan2((down - a).cos());
            a += diff * droop * 0.12;
            let step = len / segs as f32;
            let nq = (q.0 + a.cos() * step, q.1 + a.sin() * step);
            let (ra, rb) = (r0 + (r1 - r0) * t0, r0 + (r1 - r0) * t1);
            self.branches.push(Prim::capsule(q, nq, ra, rb, col0, col0));
            if depth01 > 0.99
                && self.kind.leaf != Leaf::None
                && k > 0
                && self.rng.f() < self.spec.leaves_amount.clamp(0.0, 100.0) / 100.0 * 0.9
            {
                self.add_leaf(nq, a);
            }
            q = nq;
        }
        (q, a)
    }

    fn branch(&mut self, p: (f32, f32), ang: f32, len: f32, r: f32, level: u32) {
        let levels = self.kind.levels;
        let depth01 = level as f32 / levels.max(1) as f32;
        let r_end = r * 0.62;
        let (end, a) = self.limb(p, ang, len, r, r_end, depth01);
        if level >= levels {
            let amount = self.spec.leaves_amount.clamp(0.0, 100.0) / 100.0;
            if self.kind.leaf != Leaf::None {
                let n = (amount * 9.0).round() as usize;
                for _ in 0..n {
                    let d = len * 0.5 * self.rng.f();
                    let th = self.rng.f() * std::f32::consts::TAU;
                    self.add_leaf((end.0 + th.cos() * d, end.1 + th.sin() * d), a);
                }
            }
            return;
        }
        let spread = self.kind.spread.to_radians();
        let n = self.kind.children;
        for c in 0..n {
            let side = if n == 1 {
                0.0
            } else {
                c as f32 / (n - 1) as f32 * 2.0 - 1.0
            };
            let ca = a + side * spread + self.rng.sym() * spread * 0.35;
            let cl = len * self.kind.ratio * self.rng.range(0.8, 1.1);
            let cr = r_end * self.rng.range(0.75, 0.95);
            self.branch(end, ca, cl, cr, level + 1);
        }
    }
}

/// Builds tree primitives inside `canvas`.
pub fn tree(spec: &TreeSpec, canvas: Rect) -> Vec<Prim> {
    let kind = &TREES[(spec.base_tree_type.clamp(1, TREES.len() as u32) - 1) as usize];
    let h = canvas.height() as f32 * spec.size.clamp(0.05, 2.0);
    let base = (
        canvas.x0 as f32 + canvas.width() as f32 * spec.x,
        canvas.y0 as f32 + canvas.height() as f32 * spec.y,
    );
    let la = match spec.light_direction.clamp(1, 5) {
        1 => 180.0f32,
        2 => 225.0,
        3 => 270.0,
        4 => 315.0,
        _ => 0.0,
    }
    .to_radians();
    let leaf = if spec.default_leaves {
        kind.leaf_col
    } else {
        [
            spec.leaves_color[0],
            spec.leaves_color[1],
            spec.leaves_color[2],
        ]
    };
    let bark = if spec.custom_branch_color {
        [
            spec.branches_color[0],
            spec.branches_color[1],
            spec.branches_color[2],
        ]
    } else {
        kind.bark
    };
    let thick = spec.branches_thickness.clamp(50.0, 200.0) / 100.0;
    let bh = spec.branches_height.clamp(50.0, 300.0) / 100.0;
    let mut t = TreeCtx {
        kind,
        spec,
        rng: Rng::new(spec.seed, 0x7EE + spec.base_tree_type as u64),
        branches: Vec::new(),
        leaves: Vec::new(),
        bark,
        leaf,
        light: (la.cos(), la.sin()),
        centre: (base.0, base.1 - h * 0.65),
        leaf_px: (h * 0.012 * spec.leaves_size.clamp(0.0, 200.0) / 100.0).max(0.6),
    };
    let up = -std::f32::consts::FRAC_PI_2;
    let r0 = (h * kind.thick * thick).max(0.8);
    match kind.habit {
        Habit::Branching => {
            let trunk = (h * kind.trunk * bh).min(h * 0.92);
            let (top, a) = t.limb(base, up, trunk.max(1.0), r0, r0 * 0.75, 0.0);
            // Crown length so the tree reaches about `h`.
            let geo: f32 = (0..kind.levels).map(|k| kind.ratio.powi(k as i32)).sum();
            let first = ((h - trunk).max(h * 0.08) / geo.max(0.5)) * 1.05;
            let n = kind.children.max(2);
            for c in 0..n {
                let side = c as f32 / (n - 1) as f32 * 2.0 - 1.0;
                let sp = kind.spread.to_radians() * 0.8;
                let ca = a + side * sp + t.rng.sym() * 0.15;
                let fl = first * t.rng.range(0.85, 1.05);
                t.branch(top, ca, fl, r0 * 0.7, 1);
            }
        }
        Habit::Conifer => {
            let (top, _) = t.limb(base, up, h, r0, r0 * 0.1, 0.0);
            let start = (h * kind.trunk * bh).min(h * 0.8);
            let whorls = (14.0 * (h / 300.0).sqrt().clamp(0.6, 2.5)) as usize;
            let spread = kind.spread.to_radians();
            for k in 0..whorls {
                let f = k as f32 / whorls as f32;
                let y = base.1 - start - (h - start) * f;
                let x = base.0 + (top.0 - base.0) * ((base.1 - y) / h);
                let span = (h - start) * 0.42 * (1.0 - f).powf(0.9) + h * 0.03;
                for side in [-1.0f32, 1.0] {
                    let ang = up + side * spread + t.rng.sym() * 0.1;
                    t.branch(
                        (x, y),
                        ang,
                        span,
                        (r0 * 0.35 * (1.0 - f)).max(0.6),
                        kind.levels,
                    );
                    if kind.levels >= 2 {
                        let (s2, c2) = (ang + side * 0.5).sin_cos();
                        let mid = (x + c2 * span * 0.5, y + s2 * span * 0.5);
                        t.branch(
                            mid,
                            ang + side * 0.6,
                            span * 0.4,
                            (r0 * 0.15).max(0.5),
                            kind.levels,
                        );
                    }
                }
            }
        }
        Habit::Palm => {
            let trunk = (h * kind.trunk * bh.min(1.15)).max(1.0);
            let lean = t.rng.sym() * kind.wiggle;
            let segs = 12;
            let mut p = base;
            for k in 0..segs {
                let f = k as f32 / segs as f32;
                let a = up + lean * f;
                let q = (
                    p.0 + a.cos() * trunk / segs as f32,
                    p.1 + a.sin() * trunk / segs as f32,
                );
                let r = r0 * (1.0 - 0.35 * f);
                let ring = shade(bark, if k % 2 == 0 { 1.0 } else { 0.85 });
                t.branches
                    .push(Prim::capsule(p, q, r, r * 0.97, ring, ring));
                p = q;
            }
            let fronds = kind.children;
            let flen = h * 0.38 * (spec.leaves_size.clamp(10.0, 200.0) / 100.0).sqrt();
            for k in 0..fronds {
                let a = up
                    + (k as f32 / fronds as f32 - 0.5) * std::f32::consts::PI * 1.7
                    + t.rng.sym() * 0.2;
                let mut q = p;
                let mut ang = a;
                let segs = 10;
                let leaflets =
                    (spec.leaves_amount.clamp(0.0, 100.0) / 100.0 * 3.0).round() as usize;
                for s in 0..segs {
                    // Fronds arch over and droop towards the ground.
                    let d = std::f32::consts::FRAC_PI_2 - ang;
                    ang += 0.06 * d.sin().atan2(d.cos());
                    let nq = (
                        q.0 + ang.cos() * flen / segs as f32,
                        q.1 + ang.sin() * flen / segs as f32,
                    );
                    let k2 = t.light_at(nq);
                    t.leaves.push((
                        k2 - 0.1,
                        Prim::capsule(
                            q,
                            nq,
                            (r0 * 0.15).max(0.6),
                            (r0 * 0.1).max(0.4),
                            shade(t.leaf, k2 * 0.8),
                            shade(t.leaf, k2 * 0.8),
                        ),
                    ));
                    for j in 0..leaflets {
                        let along = j as f32 / leaflets as f32;
                        let at = (q.0 + (nq.0 - q.0) * along, q.1 + (nq.1 - q.1) * along);
                        for side in [-1.0f32, 1.0] {
                            let la = ang + side * 1.1;
                            let ll = flen * 0.22 * (1.0 - s as f32 / segs as f32 * 0.7);
                            let tip = (at.0 + la.cos() * ll, at.1 + la.sin() * ll + ll * 0.25);
                            t.leaves.push((
                                k2,
                                Prim::capsule(
                                    at,
                                    tip,
                                    (ll * 0.08).max(0.5),
                                    0.3,
                                    shade(t.leaf, k2),
                                    shade(t.leaf, k2 * 0.9),
                                ),
                            ));
                        }
                    }
                    q = nq;
                }
            }
            if let Some(acc) = kind.accent {
                for _ in 0..4 {
                    let c = (p.0 + t.rng.sym() * r0 * 2.0, p.1 + t.rng.f() * r0 * 2.0);
                    t.leaves
                        .push((2.0, Prim::disc(c, r0 * 1.1, shade(acc, 1.0))));
                }
            }
        }
        Habit::Bamboo => {
            let stalks = kind.children;
            for k in 0..stalks {
                let x =
                    base.0 + (k as f32 - (stalks - 1) as f32 / 2.0) * r0 * 5.0 + t.rng.sym() * r0;
                let hh = h * t.rng.range(0.7, 1.0);
                let a = up + t.rng.sym() * kind.spread.to_radians() * 0.5;
                let joints = 8;
                let mut p = (x, base.1);
                for j in 0..joints {
                    let q = (
                        p.0 + a.cos() * hh / joints as f32,
                        p.1 + a.sin() * hh / joints as f32,
                    );
                    t.branches.push(Prim::capsule(
                        p,
                        q,
                        r0,
                        r0,
                        shade(bark, 1.0),
                        shade(bark, 0.85),
                    ));
                    t.branches.push(Prim::capsule(
                        (q.0 - r0 * 1.1, q.1),
                        (q.0 + r0 * 1.1, q.1),
                        r0 * 0.25,
                        r0 * 0.25,
                        shade(bark, 0.7),
                        shade(bark, 0.7),
                    ));
                    if j >= joints / 3 {
                        let leaves =
                            (spec.leaves_amount.clamp(0.0, 100.0) / 100.0 * 4.0).round() as usize;
                        for _ in 0..leaves {
                            let side = if t.rng.f() < 0.5 { -1.0 } else { 1.0 };
                            let la = a + side * t.rng.range(0.6, 1.4);
                            let ll = t.leaf_px * 2.2;
                            t.add_leaf((q.0 + la.cos() * ll, q.1 + la.sin() * ll), la);
                        }
                    }
                    p = q;
                }
            }
        }
    }
    // Back leaves first: darker ones underneath, lit ones on top.
    t.leaves
        .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = t.branches;
    out.extend(t.leaves.into_iter().map(|(_, p)| p));
    out
}
