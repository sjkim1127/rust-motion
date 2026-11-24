//! Interactive editing of a [`BezierMesh`]: pulling the surface and dragging control points.

use super::*;

impl BezierMesh {
    /// Pulls the surface point at normalised `(s, t)` by `d`, as dragging the grid anywhere inside
    /// a patch does in other editors. `base` is the control grid at the start of the drag.
    ///
    /// Two parts. The surrounding grid follows loosely: every interior anchor within about two cells
    /// moves by a share of `d` that fades with distance. The picture's outer border stays pinned
    /// (the picture keeps filling its frame) unless the grab is within a
    /// few percent of it, and then it follows more and more. Each anchor
    /// takes its whole 3 x 3 block of control points along, so every grid line stays smooth through
    /// its anchors and the neighbouring cells and dividers bend with the pull. Then the four inner
    /// control points of the grabbed patch make up the rest, so the grabbed point lands exactly at
    /// its start + `d`. Grabbing near an anchor moves that anchor more, and the inner points less.
    pub fn pull(&mut self, base: &[[f64; 2]], s: f64, t: f64, d: [f64; 2]) {
        /// Within this distance of the mesh border (in 0..1 box units) a grab starts to drag the
        /// border with it; further in, the border stays exactly where it is.
        const BORDER_ZONE: f64 = 0.12;
        /// Falloff radius, in the grabbed patch's size.
        const REACH: f64 = 1.8;
        if base.len() != self.points.len() || !d.iter().all(|v| v.is_finite()) {
            return;
        }
        self.points.copy_from_slice(base);
        let (cols, rows) = (self.us.len() - 1, self.vs.len() - 1);
        let (pi, a) = Self::locate(&self.us, s);
        let (pj, b) = Self::locate(&self.vs, t);
        let (ba, bb) = (bernstein(a), bernstein(b));
        let nx = self.nx();
        let anchor =
            |ai: usize, aj: usize| base.get(3 * aj * nx + 3 * ai).copied().unwrap_or([0.0; 2]);
        let at0 = self.eval(s, t);
        let target = [at0[0] + d[0], at0[1] + d[1]];
        // The grabbed patch's size and its "interior-ness": 1 in the middle, 0 on its edges.
        let mut size = 1.0f64;
        for k in 0..2 {
            let vals = [
                anchor(pi, pj)[k],
                anchor(pi + 1, pj)[k],
                anchor(pi, pj + 1)[k],
                anchor(pi + 1, pj + 1)[k],
            ];
            let (lo, hi) = vals
                .iter()
                .fold((f64::MAX, f64::MIN), |m, v| (m.0.min(*v), m.1.max(*v)));
            size = size.max(hi - lo);
        }
        let radius = REACH * size;
        let inner_w = (ba[1] + ba[2]) * (bb[1] + bb[2]);
        let interior = (inner_w / 0.5625).clamp(0.0, 1.0);
        let to_border = s.min(1.0 - s).min(t.min(1.0 - t));
        let near_border = (1.0 - to_border / BORDER_ZONE).clamp(0.0, 1.0);
        // How much of the pull each corner of the grabbed patch is responsible for.
        // A corner on the picture's border only takes part once the grab is near that border.
        // The picture's four corners never move in a pull (a grab near the border bends the
        // border through its handles instead).
        let mesh_corner = |ai: usize, aj: usize| (ai == 0 || ai == cols) && (aj == 0 || aj == rows);
        let reach = |ai: usize, aj: usize, wa: f64, wb: f64| {
            let border = ai == 0 || ai == cols || aj == 0 || aj == rows;
            if mesh_corner(ai, aj) {
                0.0
            } else {
                wa * wb * if border { near_border } else { 1.0 }
            }
        };
        let corner = [
            (pi, pj, reach(pi, pj, ba[0] + ba[1], bb[0] + bb[1])),
            (pi + 1, pj, reach(pi + 1, pj, ba[2] + ba[3], bb[0] + bb[1])),
            (pi, pj + 1, reach(pi, pj + 1, ba[0] + ba[1], bb[2] + bb[3])),
            (
                pi + 1,
                pj + 1,
                reach(pi + 1, pj + 1, ba[2] + ba[3], bb[2] + bb[3]),
            ),
        ];
        // Share of `d` for each anchor.
        let mut share = vec![0.0f64; (cols + 1) * (rows + 1)];
        for aj in 0..=rows {
            for ai in 0..=cols {
                let p = anchor(ai, aj);
                let x = ((p[0] - at0[0]).hypot(p[1] - at0[1]) / radius).clamp(0.0, 1.0);
                let fall = 0.5 * (1.0 + (std::f64::consts::PI * x).cos());
                let border = ai == 0 || ai == cols || aj == 0 || aj == rows;
                let edge = if mesh_corner(ai, aj) {
                    0.0
                } else if border {
                    near_border
                } else {
                    1.0
                };
                share[aj * (cols + 1) + ai] = fall * edge;
            }
        }
        // The grabbed patch's own corners: near one, that corner follows the pointer.
        for (ai, aj, w) in corner {
            let f = &mut share[aj * (cols + 1) + ai];
            *f = f.max((1.0 - interior) * w);
        }
        // Every control point moves with the anchor it is closest to.
        for j in 0..self.ny() {
            for i in 0..nx {
                let (ai, aj) = (((i + 1) / 3).min(cols), ((j + 1) / 3).min(rows));
                let k = share[aj * (cols + 1) + ai];
                let p = &mut self.points[j * nx + i];
                p[0] += d[0] * k;
                p[1] += d[1] * k;
            }
        }
        // Close the gap exactly, with three groups of control points that share the work:
        // - the four inner points of the grabbed patch (all of it in the middle of the patch,
        //   little near a grid line, where they barely reach the surface);
        // - the corner blocks (an anchor with its handles, moving as a whole so lines stay smooth);
        // - when the grab is near the picture's border, that border's own handles: the border
        //   bends into a curve and its corners stay put.
        // Closing nudges the tangents across the grid lines, so they are evened out again and the
        // gap re-closed.
        let norm: f64 = [1, 2]
            .iter()
            .flat_map(|j| [1, 2].iter().map(move |i| (ba[*i] * bb[*j]).powi(2)))
            .sum();
        let corner_norm: f64 = corner.iter().map(|c| c.2 * c.2).sum();
        // With no free corner to help (a one-patch grid, grabbed away from its border) the four
        // inner points do all of it.
        let inner_part = if corner_norm > 1e-12 {
            (inner_w / 0.15).min(1.0)
        } else {
            1.0
        };
        // Handles along the sides of the grabbed patch that lie on the picture's border.
        let mut edge: Vec<(usize, f64)> = Vec::new();
        for k in [1usize, 2] {
            if pi == 0 {
                edge.push(((3 * pj + k) * nx + 3 * pi, ba[0] * bb[k]));
            }
            if pi + 1 == cols {
                edge.push(((3 * pj + k) * nx + 3 * pi + 3, ba[3] * bb[k]));
            }
            if pj == 0 {
                edge.push((3 * pj * nx + 3 * pi + k, ba[k] * bb[0]));
            }
            if pj + 1 == rows {
                edge.push(((3 * pj + 3) * nx + 3 * pi + k, ba[k] * bb[3]));
            }
        }
        let edge_norm: f64 = edge.iter().map(|e| e.1 * e.1).sum();
        let edge_part = if edge_norm > 1e-12 { near_border } else { 0.0 };
        let (inner_share, corner_share) = (
            (1.0 - edge_part) * inner_part,
            (1.0 - edge_part) * (1.0 - inner_part),
        );
        for _ in 0..60 {
            let now = self.eval(s, t);
            let rem = [target[0] - now[0], target[1] - now[1]];
            if rem[0].hypot(rem[1]) < 1e-9 {
                break;
            }
            if inner_share > 0.0 && norm > 0.0 {
                for j in [1usize, 2] {
                    for i in [1usize, 2] {
                        let k = ba[i] * bb[j] / norm * inner_share;
                        let p = &mut self.points[(3 * pj + j) * nx + 3 * pi + i];
                        p[0] += rem[0] * k;
                        p[1] += rem[1] * k;
                    }
                }
            }
            if edge_part > 0.0 {
                for (idx, w) in &edge {
                    if let Some(p) = self.points.get_mut(*idx) {
                        let k = w / edge_norm * edge_part;
                        p[0] += rem[0] * k;
                        p[1] += rem[1] * k;
                    }
                }
            }
            if corner_share > 0.0 && corner_norm > 0.0 {
                for (ai, aj, w) in corner {
                    let k = w / corner_norm * corner_share;
                    for dj in -1..=1isize {
                        for di in -1..=1isize {
                            let (i, j) = ((3 * ai) as isize + di, (3 * aj) as isize + dj);
                            if i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < self.ny() {
                                let p = &mut self.points[j as usize * nx + i as usize];
                                p[0] += rem[0] * k;
                                p[1] += rem[1] * k;
                            }
                        }
                    }
                }
            }
            for _ in 0..3 {
                self.smooth_lines(
                    (3 * pi).saturating_sub(3),
                    3 * pi + 7,
                    (3 * pj).saturating_sub(3),
                    3 * pj + 7,
                );
            }
        }
    }

    /// Makes the tangent equal on both sides of every interior grid line, for the control points
    /// in columns `i0..i1` and rows `j0..j1` (exclusive ends, clamped to the grid): each pair of
    /// handles takes the average of the two tangents it had.
    fn smooth_lines(&mut self, i0: usize, i1: usize, j0: usize, j1: usize) {
        let nx = self.nx();
        let (i1, j1) = (i1.min(nx), j1.min(self.ny()));
        let hu = |k: usize| self.us[k + 1] - self.us[k];
        let hv = |k: usize| self.vs[k + 1] - self.vs[k];
        // (left, centre, right, span left, span right)
        let mut triples = Vec::new();
        for a in 1..self.us.len() - 1 {
            if 3 * a >= i0 && 3 * a < i1 {
                for j in j0..j1 {
                    triples.push((
                        j * nx + 3 * a - 1,
                        j * nx + 3 * a,
                        j * nx + 3 * a + 1,
                        hu(a - 1),
                        hu(a),
                    ));
                }
            }
        }
        for b in 1..self.vs.len() - 1 {
            if 3 * b >= j0 && 3 * b < j1 {
                for i in i0..i1 {
                    triples.push((
                        (3 * b - 1) * nx + i,
                        3 * b * nx + i,
                        (3 * b + 1) * nx + i,
                        hv(b - 1),
                        hv(b),
                    ));
                }
            }
        }
        for (l, c, r, hl, hr) in triples {
            let (pl, pc, pr) = (self.points[l], self.points[c], self.points[r]);
            let tan = [
                ((pc[0] - pl[0]) / hl + (pr[0] - pc[0]) / hr) / 2.0,
                ((pc[1] - pl[1]) / hl + (pr[1] - pc[1]) / hr) / 2.0,
            ];
            self.points[l] = [pc[0] - tan[0] * hl, pc[1] - tan[1] * hl];
            self.points[r] = [pc[0] + tan[0] * hr, pc[1] + tan[1] * hr];
        }
    }

    /// Drags control point `index` by `d` from the grid `base` it had when the drag began, keeping
    /// the surface smooth across the grid lines:
    /// - an anchor moves with the eight points around it, as one block;
    /// - a handle moves with the two points beside it across its line, and the handle opposite
    ///   it on the same anchor turns the other way (with its two neighbours), so the line keeps
    ///   passing straight through the anchor instead of creasing there.
    pub fn drag_point(&mut self, base: &[[f64; 2]], index: usize, d: [f64; 2]) {
        if base.len() != self.points.len()
            || index >= base.len()
            || !d.iter().all(|v| v.is_finite())
        {
            return;
        }
        self.points.copy_from_slice(base);
        let (nx, ny) = (self.nx(), self.ny());
        let (i, j) = (index % nx, index / nx);
        let mut shift = |i: isize, j: isize, d: [f64; 2]| {
            if i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < ny {
                let p = &mut self.points[j as usize * nx + i as usize];
                p[0] += d[0];
                p[1] += d[1];
            }
        };
        let (ii, jj) = (i as isize, j as isize);
        // Spans either side of interior knot `k`.
        let span = |knots: &[f64], k: usize| {
            (k > 0 && k + 1 < knots.len())
                .then(|| (knots[k] - knots[k - 1], knots[k + 1] - knots[k]))
        };
        match (i % 3, j % 3) {
            (0, 0) => {
                for dj in -1..=1 {
                    for di in -1..=1 {
                        shift(ii + di, jj + dj, d);
                    }
                }
            }
            // A handle along a vertical line: it takes the two points beside it across the line.
            (0, r) => {
                for di in -1..=1 {
                    shift(ii + di, jj, d);
                }
                // The opposite handle of the same anchor turns the other way.
                if let Some((hl, hr)) = span(&self.vs, j / 3 + usize::from(r == 2)) {
                    let (other, k) = if r == 1 {
                        (jj - 2, hl / hr)
                    } else {
                        (jj + 2, hr / hl)
                    };
                    for di in -1..=1 {
                        shift(ii + di, other, [-d[0] * k, -d[1] * k]);
                    }
                }
            }
            // A handle along a horizontal line, likewise.
            (r, 0) => {
                for dj in -1..=1 {
                    shift(ii, jj + dj, d);
                }
                if let Some((hl, hr)) = span(&self.us, i / 3 + usize::from(r == 2)) {
                    let (other, k) = if r == 1 {
                        (ii - 2, hl / hr)
                    } else {
                        (ii + 2, hr / hl)
                    };
                    for dj in -1..=1 {
                        shift(other, jj + dj, [-d[0] * k, -d[1] * k]);
                    }
                }
            }
            // An inner point of a patch has no handle of its own.
            _ => shift(ii, jj, d),
        }
    }
}
