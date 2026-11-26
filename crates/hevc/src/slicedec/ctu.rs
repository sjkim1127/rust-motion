//! Slice entry point, CTU loop, SAO syntax and coding quadtree.

use super::*;

impl<'a, S: Sample> SliceDecoder<'a, S> {
    #[inline(always)]
    pub(super) fn dd(&mut self, ctx: usize) -> u32 {
        self.c.decode_decision(ctx)
    }
    #[inline(always)]
    pub(super) fn bypass(&mut self) -> u32 {
        self.c.decode_bypass()
    }
    #[inline]
    pub(super) fn bypass_bits(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        let mut left = n;
        while left > 16 {
            v = (v << 16) | self.c.decode_bypass_bits(16);
            left -= 16;
        }
        // (bits beyond 32 shift out, as the bin-by-bin loop did)
        v.checked_shl(left).unwrap_or(0) | self.c.decode_bypass_bits(left)
    }

    /// Decode the slice segment `job` into `pic`.
    pub fn decode(pic: &'a mut PicState<S>, job: &'a SliceJob) -> Result<()> {
        let sh = &job.sh;
        let (sps, pps) = (&*job.sps, &*job.pps);
        ensure!(sh.data_offset < job.rbsp.len(), "empty slice segment data");
        let slice_qp = pps.init_qp + sh.qp_delta;
        let init_type = match sh.slice_type {
            SliceType::I => 0,
            SliceType::P => {
                if sh.cabac_init_flag {
                    2
                } else {
                    1
                }
            }
            SliceType::B => {
                if sh.cabac_init_flag {
                    1
                } else {
                    2
                }
            }
        };
        // SliceAddrRs: address of the first CTB of the (independent) slice
        let slice_addr = if sh.dependent {
            match pic.slices.last() {
                Some(s) => s.addr_rs,
                None => return invalid("dependent slice segment without a preceding slice"),
            }
        } else {
            sh.segment_address
        };
        if !sh.dependent {
            let ids = |l: usize| job.refs[l].iter().map(|r| r.frame.id).collect::<Vec<_>>();
            let pocs = |l: usize| job.refs[l].iter().map(|r| r.poc).collect::<Vec<_>>();
            let lts = |l: usize| job.refs[l].iter().map(|r| r.long_term).collect::<Vec<_>>();
            pic.slices.push(SliceInfo {
                addr_rs: slice_addr,
                deblock_disabled: sh.deblocking_disabled,
                beta_offset: sh.beta_offset_div2 * 2,
                tc_offset: sh.tc_offset_div2 * 2,
                lf_across: sh.loop_filter_across_slices,
                sao_luma: sh.sao_luma,
                sao_chroma: sh.sao_chroma,
                ref_ids: [ids(0), ids(1)],
                ref_pocs: [pocs(0), pocs(1)],
                ref_lt: [lts(0), lts(1)],
            });
            if !sh.deblocking_disabled {
                pic.deblocking_enabled_anywhere = true;
            }
        }
        let slice_idx = (pic.slices.len() - 1) as u32;
        let col = if sh.temporal_mvp && !sh.is_intra() {
            let l = if sh.is_b() && !sh.collocated_from_l0 {
                1
            } else {
                0
            };
            job.refs[l].get(sh.collocated_ref_idx as usize).cloned()
        } else {
            None
        };
        let no_backward_pred = job.refs.iter().flatten().all(|r| r.poc <= pic.poc);
        let log2_min_cu_qp_delta = sps.log2_ctb - pps.diff_cu_qp_delta_depth;
        let c = Cabac::new(&job.rbsp, sh.data_offset, slice_qp, init_type)?;
        let mut d = SliceDecoder {
            pic,
            sh,
            sps,
            pps,
            refs: &job.refs,
            c,
            slice_idx,
            slice_addr,
            slice_qp,
            init_type,
            qp_y: slice_qp,
            qp_y_pred: slice_qp,
            last_qp: slice_qp,
            is_cu_qp_delta_coded: false,
            cu_qp_delta: 0,
            log2_min_cu_qp_delta,
            qp_cb: 0,
            qp_cr: 0,
            cu_transquant_bypass: false,
            cu_intra: false,
            intra_chroma_mode: 0,
            max_trafo_depth: 0,
            intra_split: false,
            cu_part: PartMode::P2Nx2N,
            coeffs: vec![0; 32 * 32],
            pred0: vec![0; 64 * 64],
            pred1: vec![0; 64 * 64],
            mc: Default::default(),
            refs_buf: Refs::default(),
            col,
            no_backward_pred,
            ctb_x: 0,
            ctb_y: 0,
        };
        d.decode_data()
    }

    fn decode_data(&mut self) -> Result<()> {
        let layout = self.pic.layout.clone();
        let wctb = self.pic.wctb as u32;
        let npic = (self.pic.wctb * self.pic.hctb) as u32;
        let mut ts = layout.rs_to_ts[self.sh.segment_address as usize];
        let mut first = true;
        loop {
            let rs = layout.ts_to_rs[ts as usize];
            ensure!(
                self.pic.ctb_slice[rs as usize] == u32::MAX,
                "CTB {rs} decoded twice"
            );
            let (rx, ry) = ((rs % wctb) as i32, (rs / wctb) as i32);
            let ctb = 1i32 << self.sps.log2_ctb;
            self.ctb_x = rx * ctb;
            self.ctb_y = ry * ctb;
            self.pic.ctb_slice[rs as usize] = self.slice_idx;
            self.pic.ctb_addr[rs as usize] = self.slice_addr;
            // context initialisation / synchronisation (9.3.1, 9.3.2.1)
            let first_in_tile =
                ts == 0 || layout.tile_id[ts as usize] != layout.tile_id[ts as usize - 1];
            let wpp_row_start = self.pps.entropy_coding_sync
                && (rs.is_multiple_of(wctb)
                    || layout.tile_id[ts as usize]
                        != layout.tile_id[layout.rs_to_ts[rs as usize - 1] as usize]);
            if first_in_tile {
                if !first {
                    init_contexts(&mut self.c.ctx, self.slice_qp, self.init_type);
                }
                self.last_qp = self.slice_qp;
            } else if wpp_row_start {
                let (x0, y0) = (self.ctb_x, self.ctb_y);
                let avail = self.pic.zavail(x0, y0, x0 + ctb, y0 - ctb, self.slice_addr);
                match (&self.pic.wpp_ctx, avail) {
                    (Some(saved), true) => self.c.ctx = **saved,
                    _ => init_contexts(&mut self.c.ctx, self.slice_qp, self.init_type),
                }
                self.last_qp = self.slice_qp;
            } else if first && self.sh.dependent {
                match &self.pic.ds_ctx {
                    Some((saved, qp)) => {
                        self.c.ctx = **saved;
                        self.last_qp = *qp;
                    }
                    None => return invalid("dependent slice segment without stored contexts"),
                }
            }
            first = false;
            if self.sh.sao_luma || self.sh.sao_chroma {
                self.parse_sao(rx, ry, rs)?;
            }
            self.coding_quadtree(self.ctb_x, self.ctb_y, self.sps.log2_ctb, 0)?;
            let end = self.c.decode_terminate() == 1;
            ensure!(!self.c.overrun(), "slice data overrun");
            // WPP storage after the second CTB of a row in a tile (9.3.1)
            if self.pps.entropy_coding_sync
                && (rs % wctb == 1
                    || (rs > 1
                        && layout.tile_id[ts as usize]
                            != layout.tile_id[layout.rs_to_ts[rs as usize - 2] as usize]))
            {
                self.pic.wpp_ctx = Some(Box::new(self.c.ctx));
            }
            crate::filter::ctb_decoded(self.pic, ry as usize);
            ts += 1;
            if end {
                if self.pps.dependent_slice_segments_enabled {
                    self.pic.ds_ctx = Some((Box::new(self.c.ctx), self.last_qp));
                }
                break;
            }
            ensure!(ts < npic, "slice data continues past the last CTB");
            let nrs = layout.ts_to_rs[ts as usize];
            let new_tile = self.pps.tiles_enabled
                && layout.tile_id[ts as usize] != layout.tile_id[ts as usize - 1];
            let new_row = self.pps.entropy_coding_sync
                && (nrs.is_multiple_of(wctb)
                    || layout.tile_id[ts as usize]
                        != layout.tile_id[layout.rs_to_ts[nrs as usize - 1] as usize]);
            if new_tile || new_row {
                ensure!(self.c.decode_terminate() == 1, "end_of_subset_one_bit is 0");
                let pos = self.c.bit_pos().div_ceil(8);
                self.c.set_byte_pos(pos);
                self.c.init_engine()?;
            }
        }
        Ok(())
    }

    fn parse_sao(&mut self, rx: i32, ry: i32, rs: u32) -> Result<()> {
        let layout = self.pic.layout.clone();
        let wctb = self.pic.wctb as u32;
        let ts = layout.rs_to_ts[rs as usize];
        let mut merge_left = false;
        let mut merge_up = false;
        if rx > 0 {
            let left_in_slice = self.pic.ctb_addr[rs as usize - 1] == self.slice_addr;
            let left_in_tile = layout.tile_id[ts as usize]
                == layout.tile_id[layout.rs_to_ts[rs as usize - 1] as usize];
            if left_in_slice && left_in_tile {
                merge_left = self.dd(SAO_MERGE) == 1;
            }
        }
        if ry > 0 && !merge_left {
            let up = rs - wctb;
            let up_in_slice = self.pic.ctb_addr[up as usize] == self.slice_addr;
            let up_in_tile = layout.tile_id[ts as usize]
                == layout.tile_id[layout.rs_to_ts[up as usize] as usize];
            if up_in_slice && up_in_tile {
                merge_up = self.dd(SAO_MERGE) == 1;
            }
        }
        if merge_left {
            self.pic.sao[rs as usize] = self.pic.sao[rs as usize - 1];
            return Ok(());
        }
        if merge_up {
            self.pic.sao[rs as usize] = self.pic.sao[(rs - wctb) as usize];
            return Ok(());
        }
        let mut p = SaoParams::default();
        for c in 0..3 {
            let enabled = if c == 0 {
                self.sh.sao_luma
            } else {
                self.sh.sao_chroma
            };
            if !enabled {
                continue;
            }
            if c == 2 {
                p.type_idx[2] = p.type_idx[1];
                p.eo_class[2] = p.eo_class[1];
            } else {
                p.type_idx[c] = if self.dd(SAO_TYPE) == 0 {
                    0
                } else if self.bypass() == 0 {
                    1
                } else {
                    2
                };
            }
            if p.type_idx[c] == 0 {
                continue;
            }
            let bd = if c == 0 { self.pic.bd_y } else { self.pic.bd_c };
            let cmax = (1u32 << (bd.min(10) - 5)) - 1;
            let mut abs = [0i16; 4];
            for a in abs.iter_mut() {
                let mut v = 0;
                while v < cmax && self.bypass() == 1 {
                    v += 1;
                }
                *a = v as i16;
            }
            if p.type_idx[c] == 1 {
                for a in abs.iter_mut() {
                    if *a != 0 && self.bypass() == 1 {
                        *a = -*a;
                    }
                }
                p.band_pos[c] = self.bypass_bits(5) as u8;
                p.offsets[c] = abs;
            } else {
                p.offsets[c] = [abs[0], abs[1], -abs[2], -abs[3]];
                if c == 0 {
                    p.eo_class[0] = self.bypass_bits(2) as u8;
                }
                if c == 1 {
                    p.eo_class[1] = self.bypass_bits(2) as u8;
                }
            }
        }
        self.pic.sao[rs as usize] = p;
        Ok(())
    }

    fn coding_quadtree(&mut self, x0: i32, y0: i32, log2: u32, depth: u32) -> Result<()> {
        let size = 1i32 << log2;
        let (w, h) = (self.pic.width as i32, self.pic.height as i32);
        let split = if x0 + size <= w && y0 + size <= h && log2 > self.sps.log2_min_cb {
            // ctxInc from neighbouring depths (9.3.4.2.2)
            let mut inc = 0;
            if self.pic.zavail(x0, y0, x0 - 1, y0, self.slice_addr)
                && self.pic.blk_at(x0 - 1, y0).depth as u32 > depth
            {
                inc += 1;
            }
            if self.pic.zavail(x0, y0, x0, y0 - 1, self.slice_addr)
                && self.pic.blk_at(x0, y0 - 1).depth as u32 > depth
            {
                inc += 1;
            }
            self.dd(SPLIT_CU + inc) == 1
        } else {
            log2 > self.sps.log2_min_cb
        };
        if self.pps.cu_qp_delta_enabled && log2 >= self.log2_min_cu_qp_delta {
            self.is_cu_qp_delta_coded = false;
            self.cu_qp_delta = 0;
        }
        if split {
            let half = size >> 1;
            let (x1, y1) = (x0 + half, y0 + half);
            self.coding_quadtree(x0, y0, log2 - 1, depth + 1)?;
            if x1 < w {
                self.coding_quadtree(x1, y0, log2 - 1, depth + 1)?;
            }
            if y1 < h {
                self.coding_quadtree(x0, y1, log2 - 1, depth + 1)?;
            }
            if x1 < w && y1 < h {
                self.coding_quadtree(x1, y1, log2 - 1, depth + 1)?;
            }
            Ok(())
        } else {
            self.coding_unit(x0, y0, log2, depth)
        }
    }

    /// Set per-4x4 state of a rectangle (luma coordinates).
    pub(super) fn fill_blk(&mut self, x0: i32, y0: i32, w: i32, h: i32, f: impl Fn(&mut Blk)) {
        let (bx0, by0) = ((x0 >> 2) as usize, (y0 >> 2) as usize);
        let bx1 = (((x0 + w) as usize).min(self.pic.width).div_ceil(4)).max(bx0);
        let by1 = (((y0 + h) as usize).min(self.pic.height).div_ceil(4)).max(by0);
        for by in by0..by1 {
            for b in &mut self.pic.blk[by * self.pic.w4 + bx0..by * self.pic.w4 + bx1] {
                f(b);
            }
        }
    }

    /// qPY_PRED derivation (8.6.1) at the start of a quantization group.
    pub(super) fn derive_qp_pred(&mut self, xq: i32, yq: i32) {
        let prev = self.last_qp;
        let s = self.sps.log2_ctb;
        let cur_ctb = ((yq >> s), (xq >> s));
        let qa = if self.pic.zavail(xq, yq, xq - 1, yq, self.slice_addr)
            && ((yq >> s), ((xq - 1) >> s)) == cur_ctb
        {
            self.pic.blk_at(xq - 1, yq).qp as i32
        } else {
            prev
        };
        let qb = if self.pic.zavail(xq, yq, xq, yq - 1, self.slice_addr)
            && (((yq - 1) >> s), (xq >> s)) == cur_ctb
        {
            self.pic.blk_at(xq, yq - 1).qp as i32
        } else {
            prev
        };
        self.qp_y_pred = (qa + qb + 1) >> 1;
    }

    pub(super) fn update_qp(&mut self) {
        let off = 6 * (self.pic.bd_y as i32 - 8);
        self.qp_y = ((self.qp_y_pred + self.cu_qp_delta + 52 + 2 * off) % (52 + off)) - off;
        let offc = 6 * (self.pic.bd_c as i32 - 8);
        let qpi_cb = (self.qp_y + self.pps.cb_qp_offset + self.sh.cb_qp_offset).clamp(-offc, 57);
        let qpi_cr = (self.qp_y + self.pps.cr_qp_offset + self.sh.cr_qp_offset).clamp(-offc, 57);
        self.qp_cb = qpc_420(qpi_cb) + offc;
        self.qp_cr = qpc_420(qpi_cr) + offc;
    }
}
