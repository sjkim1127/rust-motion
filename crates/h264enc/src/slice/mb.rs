//! Slice driver, neighbour access and per-macroblock commit / rate estimation.

use super::*;

impl<'a, 'b> SliceEnc<'a, 'b> {
    pub fn new(
        f: &'a FrameEnc<'a>,
        band: Band<'b>,
        mbs: &'b mut [MbInfo],
        y0: usize,
        y1: usize,
        slice_no: u16,
    ) -> Self {
        SliceEnc {
            f,
            band,
            mbs,
            y0,
            y1,
            slice_no,
            last_qp: f.qp,
            prev_dqp_nz: false,
            cabac: None,
            cavlc: None,
            skip_run: 0,
            stats: SliceStats::default(),
        }
    }

    pub fn encode(mut self) -> SliceOut {
        let f = self.f;
        let mbw = f.p.mbw;
        let mut hdr = f.hdr.clone();
        hdr.first_mb = (self.y0 * mbw) as u32;
        let mut bw = BitWriter::new();
        hdr.write(&mut bw);
        if f.p.cabac {
            while !bw.is_byte_aligned() {
                bw.write_bit(true);
            }
            let ctx = Contexts::init(f.cabac_table, f.qp as i32);
            self.cabac = Some(CabacEncoder::new(ctx, bw.finish()));
        } else {
            self.cavlc = Some(bw);
        }
        for my in self.y0..self.y1 {
            for mx in 0..mbw {
                self.encode_mb(mx, my);
                if let Some(c) = self.cabac.as_mut() {
                    let last = my + 1 == self.y1 && mx + 1 == mbw;
                    if !last {
                        c.terminate(0);
                    }
                }
            }
        }
        let rbsp = if let Some(c) = self.cabac.take() {
            c.finish()
        } else {
            let mut w = self.cavlc.take().unwrap_or_default();
            if self.skip_run > 0 {
                w.write_ue(self.skip_run);
            }
            w.rbsp_trailing();
            w.finish()
        };
        self.stats.bits = rbsp.len() as u64 * 8;
        SliceOut {
            nal: nal(f.nal_ref_idc, f.nal_type, &rbsp),
            stats: self.stats,
        }
    }

    // ------------------------------------------------------------------ neighbour access

    #[inline]
    pub(super) fn mb(&self, mx: usize, my: usize) -> &MbInfo {
        &self.mbs[(my - self.y0) * self.f.p.mbw + mx]
    }

    fn avail(&self, mx: usize, my: usize) -> Avail {
        let top = my > self.y0;
        Avail {
            left: mx > 0,
            top,
            tr: top && mx + 1 < self.f.p.mbw,
            tl: top && mx > 0,
        }
    }

    #[inline]
    pub(super) fn ry(&self, x: usize, y: usize) -> usize {
        (y - self.y0 * 16) * self.band.ys + LUMA_PAD + x
    }
    #[inline]
    pub(super) fn rc(&self, x: usize, y: usize) -> usize {
        (y - self.y0 * 8) * self.band.cs + CHROMA_PAD + x
    }

    fn edges(&self, mx: usize, my: usize, av: Avail) -> MbEdges {
        let (px, py) = (mx * 16, my * 16);
        let mut e = MbEdges {
            top: [128; 24],
            left: [128; 16],
            tl: 128,
            ctop: [[128; 8]; 2],
            cleft: [[128; 8]; 2],
            ctl: [128; 2],
        };
        if av.top {
            let i = self.ry(px, py - 1);
            e.top[..16].copy_from_slice(&self.band.y[i..i + 16]);
            if av.tr {
                e.top[16..24].copy_from_slice(&self.band.y[i + 16..i + 24]);
            } else {
                let v = e.top[15];
                e.top[16..24].fill(v);
            }
            for (c, pl) in [&*self.band.u, &*self.band.v].into_iter().enumerate() {
                let i = self.rc(px / 2, py / 2 - 1);
                e.ctop[c].copy_from_slice(&pl[i..i + 8]);
            }
        }
        if av.left {
            for y in 0..16 {
                e.left[y] = self.band.y[self.ry(px - 1, py + y)];
            }
            for (c, pl) in [&*self.band.u, &*self.band.v].into_iter().enumerate() {
                for y in 0..8 {
                    e.cleft[c][y] = pl[self.rc(px / 2 - 1, py / 2 + y)];
                }
            }
        }
        if av.tl {
            e.tl = self.band.y[self.ry(px - 1, py - 1)];
            e.ctl[0] = self.band.u[self.rc(px / 2 - 1, py / 2 - 1)];
            e.ctl[1] = self.band.v[self.rc(px / 2 - 1, py / 2 - 1)];
        }
        e
    }

    /// Neighbouring partition data at 4x4 offset (bx, by) relative to the MB (range -1..=4).
    /// None = not available; Some((ref, mv)) with ref = -1 for intra / list unused.
    pub(super) fn nb_part(
        &self,
        cur: &MbInfo,
        mask: u16,
        mx: usize,
        my: usize,
        bx: i32,
        by: i32,
        list: usize,
    ) -> Option<(i8, Mv)> {
        if (0..4).contains(&bx) && (0..4).contains(&by) {
            let r = (by * 4 + bx) as usize;
            if mask & (1 << r) != 0 {
                return Some((cur.ref_at(list, r), cur.mv[list][r]));
            }
            return None;
        }
        let dx = if bx < 0 {
            -1
        } else if bx >= 4 {
            1
        } else {
            0
        };
        let dy = if by < 0 { -1 } else { 0 };
        if by >= 4 || (dy == 0 && dx == 1) {
            return None;
        }
        let nx = mx as i32 + dx;
        let ny = my as i32 + dy;
        if nx < 0 || nx >= self.f.p.mbw as i32 || ny < self.y0 as i32 {
            return None;
        }
        let m = self.mb(nx as usize, ny as usize);
        let r = (((by + 4) % 4) * 4 + (bx + 4) % 4) as usize;
        if m.kind.is_intra() {
            return Some((-1, Mv::ZERO));
        }
        let rf = m.ref_at(list, r);
        Some((rf, if rf >= 0 { m.mv[list][r] } else { Mv::ZERO }))
    }

    /// Motion vector predictor (§8.4.1.3) for a partition at 4x4 coords (x, y) of size (w, h) (in 4x4 units).
    pub(super) fn mvp(
        &self,
        cur: &MbInfo,
        mask: u16,
        mx: usize,
        my: usize,
        x: i32,
        y: i32,
        w: i32,
        list: usize,
        rf: i8,
        shape: Shape,
    ) -> Mv {
        let a = self.nb_part(cur, mask, mx, my, x - 1, y, list);
        let b = self.nb_part(cur, mask, mx, my, x, y - 1, list);
        let mut c = self.nb_part(cur, mask, mx, my, x + w, y - 1, list);
        if c.is_none() {
            c = self.nb_part(cur, mask, mx, my, x - 1, y - 1, list);
        }
        let (mut b, mut c) = (b, c);
        if b.is_none() && c.is_none() && a.is_some() {
            b = a;
            c = a;
        }
        let a = a.unwrap_or((-1, Mv::ZERO));
        let b = b.unwrap_or((-1, Mv::ZERO));
        let c = c.unwrap_or((-1, Mv::ZERO));
        match shape {
            Shape::Top16x8 if b.0 == rf => return b.1,
            Shape::Bottom16x8 if a.0 == rf => return a.1,
            Shape::Left8x16 if a.0 == rf => return a.1,
            Shape::Right8x16 if c.0 == rf => return c.1,
            _ => {}
        }
        let n = (a.0 == rf) as u32 + (b.0 == rf) as u32 + (c.0 == rf) as u32;
        if n == 1 {
            if a.0 == rf {
                return a.1;
            }
            if b.0 == rf {
                return b.1;
            }
            return c.1;
        }
        Mv {
            x: median(a.1.x, b.1.x, c.1.x),
            y: median(a.1.y, b.1.y, c.1.y),
        }
    }

    pub(super) fn pskip_mv(&self, mx: usize, my: usize) -> Mv {
        let cur = MbInfo::default();
        let a = self.nb_part(&cur, 0, mx, my, -1, 0, 0);
        let b = self.nb_part(&cur, 0, mx, my, 0, -1, 0);
        match (a, b) {
            (None, _) | (_, None) => Mv::ZERO,
            (Some((0, m)), _) if m == Mv::ZERO => Mv::ZERO,
            (_, Some((0, m))) if m == Mv::ZERO => Mv::ZERO,
            _ => self.mvp(&cur, 0, mx, my, 0, 0, 4, 0, 0, Shape::Normal),
        }
    }

    /// Whether a luma block at picture (x, y) of size w x h with `mv` stays within the padded reference.
    pub(super) fn mv_ok(&self, x: usize, y: usize, w: usize, h: usize, mv: Mv) -> bool {
        let pw = (self.f.p.mbw * 16) as i32;
        let ph = (self.f.p.mbh * 16) as i32;
        let m = LUMA_PAD as i32 - 20;
        let x0 = x as i32 + (mv.x as i32 >> 2);
        let y0 = y as i32 + (mv.y as i32 >> 2);
        mv.x.unsigned_abs() < 2000
            && mv.y.unsigned_abs() < 2000
            && x0 >= -m
            && y0 >= -m
            && x0 + w as i32 <= pw + m
            && y0 + h as i32 <= ph + m
    }

    // ------------------------------------------------------------------ macroblock

    fn encode_mb(&mut self, mx: usize, my: usize) {
        let f = self.f;
        let mbw = f.p.mbw;
        let aq = if f.aq.is_empty() {
            0
        } else {
            f.aq[my * mbw + mx] as i32
        };
        let qp = (f.qp as i32 + aq).clamp(0, 51) as u8;
        let av = self.avail(mx, my);
        let (px, py) = (mx * 16, my * 16);
        let mut src_y = [0u8; 256];
        let mut src_u = [0u8; 64];
        let mut src_v = [0u8; 64];
        {
            let s = &f.src.y;
            for r in 0..16 {
                let i = s.idx(px as isize, (py + r) as isize);
                src_y[r * 16..r * 16 + 16].copy_from_slice(&s.data[i..i + 16]);
            }
            for r in 0..8 {
                let i = f.src.u.idx((px / 2) as isize, (py / 2 + r) as isize);
                src_u[r * 8..r * 8 + 8].copy_from_slice(&f.src.u.data[i..i + 8]);
                let i = f.src.v.idx((px / 2) as isize, (py / 2 + r) as isize);
                src_v[r * 8..r * 8 + 8].copy_from_slice(&f.src.v.data[i..i + 8]);
            }
        }
        let src = Src {
            y: src_y,
            u: src_u,
            v: src_v,
        };
        let edges = self.edges(mx, my, av);
        let best = match f.st {
            SliceType::I => self.analyse_intra(mx, my, qp, av, &edges, &src, u64::MAX),
            SliceType::P => self.analyse_p(mx, my, qp, av, &edges, &src),
            SliceType::B => self.analyse_b(mx, my, qp, av, &edges, &src),
        };
        self.commit(mx, my, qp, *best);
    }

    fn commit(&mut self, mx: usize, my: usize, qp: u8, mut c: Cand) {
        let (px, py) = (mx * 16, my * 16);
        // reconstruction
        for r in 0..16 {
            let i = self.ry(px, py + r);
            self.band.y[i..i + 16].copy_from_slice(&c.ry[r * 16..r * 16 + 16]);
        }
        for r in 0..8 {
            let i = self.rc(px / 2, py / 2 + r);
            self.band.u[i..i + 8].copy_from_slice(&c.ru[r * 8..r * 8 + 8]);
            self.band.v[i..i + 8].copy_from_slice(&c.rv[r * 8..r * 8 + 8]);
        }
        let kind = c.code.kind;
        let has_dqp = !kind.is_skip() && (c.code.cbp != 0 || kind == MbKind::I16x16);
        let prev_dqp_nz = self.prev_dqp_nz;
        if has_dqp {
            let mut d = qp as i32 - self.last_qp as i32;
            if d < -26 {
                d += 52;
            } else if d > 25 {
                d -= 52;
            }
            c.code.qp_delta = d;
            self.prev_dqp_nz = d != 0;
            self.last_qp = qp;
        } else {
            c.code.qp_delta = 0;
            self.prev_dqp_nz = false;
        }
        c.info.qp = self.last_qp;
        c.info.slice = self.slice_no + 1;
        c.info.kind = kind;
        c.info.cbp = if kind.is_skip() { 0 } else { c.code.cbp };
        self.stats.mbs += 1;
        self.stats.qp_sum += c.info.qp as u64;
        if kind.is_intra() {
            self.stats.intra += 1;
        } else if kind.is_skip() {
            self.stats.skip += 1;
        } else {
            self.stats.inter += 1;
        }
        let idx = (my - self.y0) * self.f.p.mbw + mx;
        self.mbs[idx] = c.info;
        // entropy coding
        let av = self.avail(mx, my);
        let left = if av.left {
            Some(&self.mbs[idx - 1])
        } else {
            None
        };
        let top = if av.top {
            Some(&self.mbs[idx - self.f.p.mbw])
        } else {
            None
        };
        let nb = NbCtx {
            left,
            top,
            cur: &self.mbs[idx],
            slice_type: self.f.st,
            prev_dqp_nz,
            t8x8_mode: self.f.p.t8x8,
        };
        if let Some(cab) = self.cabac.as_mut() {
            if nb.slice_type != SliceType::I {
                cabac_mb::write_skip_flag(cab, &nb, kind.is_skip());
            }
            if !kind.is_skip() {
                cabac_mb::write_mb(cab, &nb, &c.code);
            }
        } else if let Some(w) = self.cavlc.as_mut() {
            if kind.is_skip() {
                self.skip_run += 1;
            } else {
                if nb.slice_type != SliceType::I {
                    w.write_ue(self.skip_run);
                    self.skip_run = 0;
                }
                cavlc::write_mb(w, &nb, &c.code);
            }
        }
    }

    /// Estimated bits (1/256 units) of coding candidate `c` at the current entropy state.
    fn est_bits(&self, mx: usize, my: usize, c: &Cand, qp: u8) -> u64 {
        let av = self.avail(mx, my);
        let idx = (my - self.y0) * self.f.p.mbw + mx;
        let left = if av.left {
            Some(&self.mbs[idx - 1])
        } else {
            None
        };
        let top = if av.top {
            Some(&self.mbs[idx - self.f.p.mbw])
        } else {
            None
        };
        let mut info = c.info;
        info.kind = c.code.kind;
        info.cbp = c.code.cbp;
        let mut code = c.code.clone();
        code.qp_delta = qp as i32 - self.last_qp as i32;
        let nb = NbCtx {
            left,
            top,
            cur: &info,
            slice_type: self.f.st,
            prev_dqp_nz: self.prev_dqp_nz,
            t8x8_mode: self.f.p.t8x8,
        };
        if let Some(cab) = self.cabac.as_ref() {
            let mut est = CabacEstimator::new(cab.ctx.clone(), self.f.cost_tab);
            if nb.slice_type != SliceType::I {
                cabac_mb::write_skip_flag(&mut est, &nb, code.kind.is_skip());
            }
            if !code.kind.is_skip() {
                cabac_mb::write_mb(&mut est, &nb, &code);
            }
            est.cost
        } else {
            if code.kind.is_skip() {
                return 256;
            }
            let mut w = BitWriter::new();
            if nb.slice_type != SliceType::I {
                w.write_ue(self.skip_run);
            }
            cavlc::write_mb(&mut w, &nb, &code);
            w.bit_len() as u64 * 256
        }
    }

    pub(super) fn rd_cost(&self, mx: usize, my: usize, c: &Cand, src: &Src, qp: u8) -> u64 {
        let d = ssd(&src.y, 16, &c.ry, 16, 16, 16)
            + ssd(&src.u, 8, &c.ru, 8, 8, 8)
            + ssd(&src.v, 8, &c.rv, 8, 8, 8);
        let bits = self.est_bits(mx, my, c, qp);
        let lam = self.f.lam.ssd[qp as usize];
        (d as f64 * 256.0 + lam * bits as f64) as u64
    }
}
