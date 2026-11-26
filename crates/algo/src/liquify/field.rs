//! The displacement field and the dab walk that edits it.

use super::*;

/// The displacement field plus the freeze mask, over `bounds` (document pixels).
#[derive(Clone, Debug)]
pub struct LiquifyField {
    pub bounds: Rect,
    /// Document pixels per field node.
    pub cell: f64,
    pub w: usize,
    pub h: usize,
    /// Inverse displacement per node, row-major.
    pub d: Vec<[f32; 2]>,
    /// Freeze mask per node (0 = free, 1 = frozen).
    pub freeze: Vec<f32>,
    /// Where the brush stroke in progress stands, so dab spacing runs on across its segments.
    cursor: Option<StrokeCursor>,
}

/// Two fields are equal when their displacement and freeze mask are: the stroke cursor is
/// transient brush state, not part of the result.
impl PartialEq for LiquifyField {
    fn eq(&self, o: &Self) -> bool {
        self.bounds == o.bounds
            && self.cell == o.cell
            && self.w == o.w
            && self.h == o.h
            && self.d == o.d
            && self.freeze == o.freeze
    }
}

/// The brush stroke in progress: the point its last segment ended at, its last dab and how far
/// the brush has travelled along the path since that dab.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StrokeCursor {
    end: [f64; 2],
    last_dab: [f64; 2],
    carry: f64,
}

/// Distance between dabs along a stroke for brush radius `r`: a twentieth of the radius, so a
/// drag is many small pushes and its edge stays smooth.
fn dab_spacing(r: f64) -> f64 {
    (r * 0.05).max(0.5)
}

/// Per-dab strength of a stationary tool (Twirl, Pucker, Bloat, Reconstruct, Smooth, Freeze,
/// Thaw) while it moves: scaled to the spacing so its effect per distance travelled stays what it
/// was with dabs a fifth of the radius apart.
fn moving_weight(r: f64) -> f64 {
    dab_spacing(r) / (r * 0.2).max(0.5)
}

/// The default field resolution for `bounds`: 2 px per node up to 4 MP, 4 px beyond.
pub fn auto_cell(bounds: Rect) -> f64 {
    if bounds.size().area() <= 4_200_000 {
        2.0
    } else {
        4.0
    }
}

impl LiquifyField {
    pub fn new(bounds: Rect, cell: f64) -> Self {
        let cell = cell.clamp(1.0, 64.0);
        let w = (f64::from(bounds.width()) / cell).ceil() as usize + 2;
        let h = (f64::from(bounds.height()) / cell).ceil() as usize + 2;
        LiquifyField {
            bounds,
            cell,
            w,
            h,
            d: vec![[0.0; 2]; w * h],
            freeze: vec![0.0; w * h],
            cursor: None,
        }
    }

    /// Document position of node (i, j) (node 0 sits on the first pixel centre).
    #[inline]
    pub fn node_pos(&self, i: usize, j: usize) -> [f64; 2] {
        [
            f64::from(self.bounds.x0) + 0.5 + i as f64 * self.cell,
            f64::from(self.bounds.y0) + 0.5 + j as f64 * self.cell,
        ]
    }

    /// Field coordinates of a document point.
    #[inline]
    pub(super) fn to_grid(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - f64::from(self.bounds.x0) - 0.5) / self.cell,
            (y - f64::from(self.bounds.y0) - 0.5) / self.cell,
        )
    }

    pub fn is_identity(&self) -> bool {
        self.d.iter().all(|v| v[0] == 0.0 && v[1] == 0.0)
    }

    /// Largest displacement magnitude (px).
    pub fn max_displacement(&self) -> f32 {
        self.d.iter().map(|v| v[0].hypot(v[1])).fold(0.0, f32::max)
    }

    /// Bilinear displacement at a document point (zero outside the field).
    pub fn sample(&self, x: f64, y: f64) -> [f64; 2] {
        let (gx, gy) = self.to_grid(x, y);
        bilinear(&self.d, self.w, self.h, gx, gy)
    }

    /// Freeze value at a document point (bilinear).
    pub fn freeze_at(&self, x: f64, y: f64) -> f32 {
        let (gx, gy) = self.to_grid(x, y);
        let gx = gx.clamp(0.0, (self.w - 1) as f64);
        let gy = gy.clamp(0.0, (self.h - 1) as f64);
        let (i, j) = ((gx as usize).min(self.w - 2), (gy as usize).min(self.h - 2));
        let (fx, fy) = ((gx - i as f64) as f32, (gy - j as f64) as f32);
        let at = |i: usize, j: usize| self.freeze[j * self.w + i];
        (at(i, j) * (1.0 - fx) + at(i + 1, j) * fx) * (1.0 - fy)
            + (at(i, j + 1) * (1.0 - fx) + at(i + 1, j + 1) * fx) * fy
    }

    /// Applies one stroke. Returns the document rectangle whose output changed.
    pub fn apply_stroke(&mut self, s: &LiquifyStroke) -> Rect {
        self.cursor = None;
        match s.tool {
            LiquifyTool::LassoMask => return self.apply_lasso(s),
            LiquifyTool::ReconstructAll => {
                let k = (s.amount.unwrap_or(100.0) / 100.0).clamp(0.0, 1.0) as f32;
                for (v, f) in self.d.iter_mut().zip(&self.freeze) {
                    let m = 1.0 - k * (1.0 - f);
                    v[0] *= m;
                    v[1] *= m;
                }
                return self.bounds;
            }
            LiquifyTool::FreezeAll => self.freeze.fill(1.0),
            LiquifyTool::ThawAll => self.freeze.fill(0.0),
            LiquifyTool::InvertFreeze => self.freeze.iter_mut().for_each(|f| *f = 1.0 - *f),
            _ => {}
        }
        if s.tool.is_global() {
            return self.bounds;
        }
        let pts: Vec<[f64; 3]> = s
            .points
            .iter()
            .filter(|p| p.len() >= 2)
            .map(|p| [p[0], p[1], p.get(2).copied().unwrap_or(1.0)])
            .collect();
        let Some(&first) = pts.first() else {
            return Rect::EMPTY;
        };
        let mut dirty = self.stroke_begin(s, first);
        for w in pts.windows(2) {
            dirty = dirty.union(&self.stroke_segment(s, w[0], w[1]));
        }
        dirty
    }

    /// Lasso: sets the freeze mask to `amount` (1.0 when none; 0.0 thaws) at every field node
    /// inside the closed polygon in `s.points` (even-odd rule; non-finite points are skipped).
    /// A scanline fill: each node row costs one pass over the edges, then only the nodes inside
    /// are written. Returns the document rectangle the mask changed in.
    fn apply_lasso(&mut self, s: &LiquifyStroke) -> Rect {
        let poly: Vec<[f64; 2]> = s
            .points
            .iter()
            .filter_map(|p| Some([*p.first()?, *p.get(1)?]))
            .filter(|p| p[0].is_finite() && p[1].is_finite())
            .collect();
        if poly.len() < 3 || self.w == 0 || self.h == 0 {
            return Rect::EMPTY;
        }
        let (mut px0, mut py0, mut px1, mut py1) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for p in &poly {
            px0 = px0.min(p[0]);
            py0 = py0.min(p[1]);
            px1 = px1.max(p[0]);
            py1 = py1.max(p[1]);
        }
        let (_, gy0) = self.to_grid(px0, py0);
        let (_, gy1) = self.to_grid(px1, py1);
        let last_row = self.h.saturating_sub(1) as f64;
        let last_col = self.w.saturating_sub(1) as f64;
        let (j0, j1) = (gy0.ceil().max(0.0), gy1.floor().min(last_row));
        if j1 < j0 {
            return Rect::EMPTY;
        }
        let v = if s.amount.unwrap_or(1.0) > 0.5 {
            1.0f32
        } else {
            0.0
        };
        let mut xs: Vec<f64> = Vec::new();
        for j in (j0 as usize)..=(j1 as usize) {
            let y = self.node_pos(0, j)[1];
            xs.clear();
            let Some(mut prev) = poly.last().copied() else {
                return Rect::EMPTY;
            };
            for &a in &poly {
                if (a[1] > y) != (prev[1] > y) {
                    xs.push(a[0] + (y - a[1]) * (prev[0] - a[0]) / (prev[1] - a[1]));
                }
                prev = a;
            }
            xs.sort_by(f64::total_cmp);
            // Even-odd: nodes with xs[2k] <= x < xs[2k + 1] are inside.
            for &[xa, xb] in xs.as_chunks::<2>().0 {
                let (ga, _) = self.to_grid(xa, y);
                let (gb, _) = self.to_grid(xb, y);
                let (i0, i1) = (ga.ceil().max(0.0), (gb.ceil() - 1.0).min(last_col));
                if i1 < i0 {
                    continue;
                }
                let row = j * self.w;
                if let Some(cells) = self.freeze.get_mut(row + i0 as usize..=row + i1 as usize) {
                    cells.fill(v);
                }
            }
        }
        Rect::new(
            px0.floor() as i32,
            py0.floor() as i32,
            px1.ceil() as i32,
            py1.ceil() as i32,
        )
        .intersect(&self.bounds)
    }

    /// Starts a stroke at `p` (`[x, y, pressure]`): stationary tools dab once there. Interactive
    /// callers use this plus [`Self::stroke_segment`] per new point, which is exactly what
    /// [`Self::apply_stroke`] does with the whole point list.
    pub fn stroke_begin(&mut self, s: &LiquifyStroke, p: [f64; 3]) -> Rect {
        self.cursor = Some(StrokeCursor {
            end: [p[0], p[1]],
            last_dab: [p[0], p[1]],
            carry: 0.0,
        });
        if s.tool.is_stationary() && !s.tool.is_global() {
            self.dab(s, [p[0], p[1]], [0.0, 0.0], p[2].clamp(0.0, 1.0), 1.0)
        } else {
            Rect::EMPTY
        }
    }

    /// Continues a stroke from `a` to `b`. Dabs fall every [`dab_spacing`] of distance travelled
    /// along the whole stroke, carried from one segment to the next, so the result is the same
    /// however finely the pointer path was sampled (a fast and a slow mouse give one warp). A
    /// stationary tool held still dabs once at `b`. When `a` is not where the previous segment
    /// ended, the stroke starts afresh at `a`.
    pub fn stroke_segment(&mut self, s: &LiquifyStroke, a: [f64; 3], b: [f64; 3]) -> Rect {
        self.walk_segment(s, a, b, true)
    }

    /// [`Self::stroke_segment`]; `skip` leaves out the dabs that cannot reach a field node (they
    /// would change nothing), which tests compare with the full walk.
    pub(super) fn walk_segment(
        &mut self,
        s: &LiquifyStroke,
        a: [f64; 3],
        b: [f64; 3],
        skip: bool,
    ) -> Rect {
        let mut dirty = Rect::EMPTY;
        let (pa, pb) = (a[2].clamp(0.0, 1.0), b[2].clamp(0.0, 1.0));
        let stationary = s.tool.is_stationary();
        let cur = match self.cursor {
            Some(c) if c.end == [a[0], a[1]] => c,
            _ => StrokeCursor {
                end: [a[0], a[1]],
                last_dab: [a[0], a[1]],
                carry: 0.0,
            },
        };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        if len == 0.0 {
            self.cursor = Some(cur);
            return if stationary {
                self.dab(s, [b[0], b[1]], [0.0, 0.0], pb, 1.0)
            } else {
                dirty
            };
        }
        // NaN or a length that overflowed: nothing sensible to walk; the next segment restarts.
        if !len.is_finite() {
            self.cursor = None;
            return dirty;
        }
        let r = (s.size / 2.0).max(0.5);
        let spacing = dab_spacing(r);
        let weight = if stationary { moving_weight(r) } else { 1.0 };
        // Distance along this segment of its first dab, then one every `spacing`.
        let first = (spacing - cur.carry).clamp(0.0, spacing);
        // Distances are compared with a little slack: a dab that rounding puts a hair past the
        // segment's end still falls on it, so cutting one segment in two never adds or drops a dab.
        let slack = spacing * 1e-6;
        if first > len + slack {
            self.cursor = Some(StrokeCursor {
                end: [b[0], b[1]],
                last_dab: cur.last_dab,
                carry: cur.carry + len,
            });
            return dirty;
        }
        let last_k = ((len - first + slack) / spacing).floor().max(0.0);
        let at = |dist: f64| [a[0] + dx * dist / len, a[1] + dy * dist / len];
        let (mut k0, mut k1) = (0.0f64, last_k);
        if skip {
            match self.reachable_span(a, [dx, dy], len, r) {
                // A dragging dab is centred one spacing behind its position: pad by that and a step.
                Some((d0, d1)) => {
                    k0 = k0.max(((d0 - spacing - first) / spacing).floor() - 1.0);
                    k1 = k1.min(((d1 + spacing - first) / spacing).ceil() + 1.0);
                }
                None => k1 = -1.0,
            }
        }
        if k0 <= k1 {
            let mut prev = if k0 > 0.0 {
                at(first + (k0 - 1.0) * spacing)
            } else {
                cur.last_dab
            };
            for k in (k0 as u64)..=(k1 as u64) {
                let dist = (first + k as f64 * spacing).min(len);
                let c = at(dist);
                let delta = [c[0] - prev[0], c[1] - prev[1]];
                // Forward Warp centres the dab where the brush was (it drags what is under it).
                let centre = if stationary { c } else { prev };
                dirty =
                    dirty.union(&self.dab(s, centre, delta, pa + (pb - pa) * dist / len, weight));
                prev = c;
            }
        }
        let last = (first + last_k * spacing).min(len);
        self.cursor = Some(StrokeCursor {
            end: [b[0], b[1]],
            last_dab: at(last),
            carry: (len - last).clamp(0.0, spacing),
        });
        dirty
    }

    /// The distances along a segment from `a` (direction `dir`, length `len`) whose points lie
    /// within a dab radius `r` plus two nodes of the node grid, or `None` when none do. Only dabs
    /// near that span can touch a node; a segment to a far-off point has trillions of others
    /// (#937).
    fn reachable_span(&self, a: [f64; 3], dir: [f64; 2], len: f64, r: f64) -> Option<(f64, f64)> {
        if self.w == 0 || self.h == 0 {
            return None;
        }
        let pad = r + 2.0 * self.cell;
        let lo = [
            f64::from(self.bounds.x0) + 0.5 - pad,
            f64::from(self.bounds.y0) + 0.5 - pad,
        ];
        let hi = [
            lo[0] + (self.w - 1) as f64 * self.cell + 2.0 * pad,
            lo[1] + (self.h - 1) as f64 * self.cell + 2.0 * pad,
        ];
        let (mut u0, mut u1) = (0.0f64, 1.0f64);
        for ax in 0..2 {
            let d = dir[ax];
            if d == 0.0 {
                if a[ax].is_nan() || a[ax] < lo[ax] || a[ax] > hi[ax] {
                    return None;
                }
                continue;
            }
            let (v0, v1) = ((lo[ax] - a[ax]) / d, (hi[ax] - a[ax]) / d);
            u0 = u0.max(v0.min(v1));
            u1 = u1.min(v0.max(v1));
        }
        if u0.is_nan() || u1.is_nan() || u0 > u1 {
            return None;
        }
        Some((u0 * len, u1 * len))
    }

    /// One dab at `c` with brush motion `delta`, its strength scaled by `weight`. Returns the
    /// document rect it affected.
    fn dab(
        &mut self,
        s: &LiquifyStroke,
        c: [f64; 2],
        delta: [f64; 2],
        point_pressure: f64,
        weight: f64,
    ) -> Rect {
        if s.tool == LiquifyTool::LassoMask {
            return Rect::EMPTY; // the lasso is polygon-only; never a brush dab
        }
        let r = (s.size / 2.0).max(0.5);
        let strength = (s.pressure / 100.0).clamp(0.0, 1.0) * point_pressure * weight;
        let rate = (s.rate / 100.0).clamp(0.0, 1.0);
        let density = (s.density / 100.0).clamp(0.0, 1.0);
        if strength <= 0.0 {
            return Rect::EMPTY;
        }
        let (gx0, gy0) = self.to_grid(c[0] - r, c[1] - r);
        let (gx1, gy1) = self.to_grid(c[0] + r, c[1] + r);
        let i0 = gx0.floor().max(0.0) as usize;
        let j0 = gy0.floor().max(0.0) as usize;
        let i1 = (gx1.ceil().max(-1.0) as isize).min(self.w as isize - 1);
        let j1 = (gy1.ceil().max(-1.0) as isize).min(self.h as isize - 1);
        if i1 < i0 as isize || j1 < j0 as isize {
            return Rect::EMPTY;
        }
        let (i1, j1) = (i1 as usize, j1 as usize);
        // Snapshot of the affected nodes plus a margin (sampling reads around them).
        let m = ((delta[0].abs().max(delta[1].abs()) + r * 0.2) / self.cell).ceil() as usize + 2;
        let (si0, sj0) = (i0.saturating_sub(m), j0.saturating_sub(m));
        let (si1, sj1) = ((i1 + m).min(self.w - 1), (j1 + m).min(self.h - 1));
        let sw = si1 - si0 + 1;
        let sh = sj1 - sj0 + 1;
        let mut snap = Vec::with_capacity(sw * sh);
        for j in sj0..=sj1 {
            snap.extend_from_slice(&self.d[j * self.w + si0..=j * self.w + si1]);
        }
        let old = |gx: f64, gy: f64| -> [f64; 2] {
            let (lx, ly) = (gx - si0 as f64, gy - sj0 as f64);
            if lx >= 0.0 && ly >= 0.0 && lx <= (sw - 1) as f64 && ly <= (sh - 1) as f64 {
                bilinear(&snap, sw, sh, lx, ly)
            } else {
                // Outside the snapshot nothing changed: read the live field.
                bilinear(&self.d, self.w, self.h, gx, gy)
            }
        };
        let mut writes: Vec<(usize, [f32; 2], f32)> =
            Vec::with_capacity((i1 - i0 + 1) * (j1 - j0 + 1));
        let inv_cell = 1.0 / self.cell;
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = self.node_pos(i, j);
                let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
                let t2 = (dx * dx + dy * dy) / (r * r);
                if t2 >= 1.0 {
                    continue;
                }
                let fall = falloff(t2, density);
                let idx = j * self.w + i;
                let k_raw = strength * fall;
                if s.tool == LiquifyTool::Freeze || s.tool == LiquifyTool::Thaw {
                    let f = self.freeze[idx];
                    let v = if s.tool == LiquifyTool::Freeze {
                        (f as f64 + k_raw).min(1.0)
                    } else {
                        (f as f64 - k_raw).max(0.0)
                    };
                    writes.push((idx, self.d[idx], v as f32));
                    continue;
                }
                let k = k_raw * (1.0 - f64::from(self.freeze[idx]));
                if k <= 0.0 {
                    continue;
                }
                let cur = self.d[idx];
                let new = match s.tool {
                    LiquifyTool::Reconstruct => {
                        let a = (1.0 - k * (0.1 + 0.4 * rate)) as f32;
                        [cur[0] * a, cur[1] * a]
                    }
                    LiquifyTool::Smooth => {
                        let (gx, gy) = ((i - si0) as isize, (j - sj0) as isize);
                        let at = |x: isize, y: isize| {
                            snap[(y.clamp(0, sh as isize - 1) as usize) * sw
                                + x.clamp(0, sw as isize - 1) as usize]
                        };
                        let mut mean = [0.0f32; 2];
                        for (ox, oy) in [
                            (-1, 0),
                            (1, 0),
                            (0, -1),
                            (0, 1),
                            (-1, -1),
                            (1, -1),
                            (-1, 1),
                            (1, 1),
                        ] {
                            let v = at(gx + ox, gy + oy);
                            mean[0] += v[0] / 8.0;
                            mean[1] += v[1] / 8.0;
                        }
                        let a = (k * (0.1 + 0.4 * rate)) as f32;
                        [
                            cur[0] + (mean[0] - cur[0]) * a,
                            cur[1] + (mean[1] - cur[1]) * a,
                        ]
                    }
                    _ => {
                        let q = match s.tool {
                            LiquifyTool::ForwardWarp => [p[0] - k * delta[0], p[1] - k * delta[1]],
                            // Left of the stroke direction (y down): (dx, dy) → (dy, -dx).
                            LiquifyTool::PushLeft => [p[0] - k * delta[1], p[1] + k * delta[0]],
                            LiquifyTool::TwirlCw | LiquifyTool::TwirlCcw => {
                                let th = k
                                    * rate
                                    * 0.12
                                    * if s.tool == LiquifyTool::TwirlCw {
                                        1.0
                                    } else {
                                        -1.0
                                    };
                                // Content turns by +th, so sample at the point turned by -th.
                                let (sn, cs) = (-th).sin_cos();
                                [c[0] + dx * cs - dy * sn, c[1] + dx * sn + dy * cs]
                            }
                            LiquifyTool::Pucker => {
                                let a = 1.0 + k * rate * 0.06;
                                [c[0] + dx * a, c[1] + dy * a]
                            }
                            _ => {
                                let a = 1.0 - k * rate * 0.06;
                                [c[0] + dx * a, c[1] + dy * a]
                            }
                        };
                        let (qx, qy) = (
                            (q[0] - p[0]) * inv_cell + i as f64,
                            (q[1] - p[1]) * inv_cell + j as f64,
                        );
                        let dq = old(qx, qy);
                        [(q[0] + dq[0] - p[0]) as f32, (q[1] + dq[1] - p[1]) as f32]
                    }
                };
                writes.push((idx, new, self.freeze[idx]));
            }
        }
        for (idx, d, f) in writes {
            self.d[idx] = d;
            self.freeze[idx] = f;
        }
        let pad = r.ceil() as i32 + 2;
        Rect::new(
            c[0].floor() as i32 - pad,
            c[1].floor() as i32 - pad,
            c[0].ceil() as i32 + pad,
            c[1].ceil() as i32 + pad,
        )
        .intersect(&self.bounds)
    }

    /// Replays strokes on a fresh field.
    pub fn from_strokes(bounds: Rect, cell: f64, strokes: &[LiquifyStroke]) -> Self {
        let mut f = LiquifyField::new(bounds, cell);
        for s in strokes {
            f.apply_stroke(s);
        }
        f
    }

    /// Catmull-Rom (bicubic) displacement at grid coordinates.
    pub(super) fn sample_cubic(&self, gx: f64, gy: f64, wx: &[f64; 4], wy: &[f64; 4]) -> [f64; 2] {
        let (ix, iy) = (gx.floor() as isize, gy.floor() as isize);
        let mut acc = [0.0f64; 2];
        for (jj, wyj) in wy.iter().enumerate() {
            let y = (iy - 1 + jj as isize).clamp(0, self.h as isize - 1) as usize;
            let row = &self.d[y * self.w..(y + 1) * self.w];
            for (ii, wxi) in wx.iter().enumerate() {
                let x = (ix - 1 + ii as isize).clamp(0, self.w as isize - 1) as usize;
                let k = wxi * wyj;
                acc[0] += f64::from(row[x][0]) * k;
                acc[1] += f64::from(row[x][1]) * k;
            }
        }
        acc
    }

    /// Whether any node influencing document rect `r` is displaced.
    pub(super) fn touches(&self, r: Rect) -> bool {
        let (gx0, gy0) = self.to_grid(f64::from(r.x0), f64::from(r.y0));
        let (gx1, gy1) = self.to_grid(f64::from(r.x1), f64::from(r.y1));
        let i0 = (gx0.floor() as isize - 2).clamp(0, self.w as isize - 1) as usize;
        let j0 = (gy0.floor() as isize - 2).clamp(0, self.h as isize - 1) as usize;
        let i1 = (gx1.ceil() as isize + 2).clamp(0, self.w as isize - 1) as usize;
        let j1 = (gy1.ceil() as isize + 2).clamp(0, self.h as isize - 1) as usize;
        (j0..=j1).any(|j| {
            self.d[j * self.w + i0..=j * self.w + i1]
                .iter()
                .any(|v| v[0] != 0.0 || v[1] != 0.0)
        })
    }
}
