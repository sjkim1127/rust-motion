//! Slice-encoder data types: parameters, per-frame inputs, candidates and the `SliceEnc` state.

use super::*;

/// Encoder tuning derived from the preset.
#[derive(Clone, Debug)]
pub struct EncParams {
    pub mbw: usize,
    pub mbh: usize,
    pub cabac: bool,
    pub t8x8: bool,
    pub i4x4: bool,
    pub i8x8: bool,
    pub partitions: bool,
    pub me_range: i32,
    /// 0 = full-pel, 1 = half-pel, 2 = quarter-pel refinement.
    pub subpel: u8,
    /// Extra quarter-pel refinement rounds.
    pub subpel_rounds: u8,
    pub rd: bool,
    pub chroma_qp_offset: i32,
    pub decimate: bool,
    /// Adaptive 4x4/8x8 transform decision for inter macroblocks.
    pub adaptive_t8: bool,
    /// Always try intra in P/B frames (otherwise only when inter looks poor).
    pub always_intra: bool,
    /// Maximum CAVLC level magnitude (Baseline).
    pub cavlc_clamp: bool,
}

/// Per-frame shared encoding state.
pub struct FrameEnc<'a> {
    pub p: &'a EncParams,
    pub qt: &'a QuantTables,
    pub cost_tab: &'a CostTable,
    pub st: SliceType,
    pub src: &'a Frame,
    pub l0: Option<&'a RefPic>,
    pub l1: Option<&'a RefPic>,
    pub qp: u8,
    pub aq: &'a [i8],
    pub hdr: SliceHeader,
    pub nal_ref_idc: u8,
    pub nal_type: u8,
    pub cabac_table: &'a [(i8, i8); NUM_CTX],
    pub lam: &'a Lambdas,
}

/// Lambda tables indexed by QP.
pub struct Lambdas {
    /// SATD/SAD-domain lambda, Q8.
    pub sad: [u32; 52],
    /// SSD-domain lambda (for RD), Q8 per 1/256 bit.
    pub ssd: [f64; 52],
}

impl Lambdas {
    pub fn new() -> Self {
        let mut sad = [0u32; 52];
        let mut ssd_ = [0f64; 52];
        for qp in 0..52 {
            let l = 0.85 * 2f64.powf((qp as f64 - 12.0) / 3.0);
            ssd_[qp] = l;
            sad[qp] = ((l.sqrt()) * 256.0).round().max(16.0) as u32;
        }
        Lambdas { sad, ssd: ssd_ }
    }
}

impl Default for Lambdas {
    fn default() -> Self {
        Self::new()
    }
}

/// Mutable band of reconstructed rows belonging to one slice.
pub struct Band<'b> {
    pub y: &'b mut [u8],
    pub u: &'b mut [u8],
    pub v: &'b mut [u8],
    pub ys: usize,
    pub cs: usize,
}

#[derive(Default, Clone, Debug)]
pub struct SliceStats {
    pub bits: u64,
    pub intra: u32,
    pub skip: u32,
    pub inter: u32,
    pub qp_sum: u64,
    pub mbs: u32,
}

pub struct SliceOut {
    pub nal: Vec<u8>,
    #[allow(dead_code)]
    pub stats: SliceStats,
}

#[derive(Clone, Copy)]
pub(super) struct Avail {
    pub(super) left: bool,
    pub(super) top: bool,
    pub(super) tr: bool,
    pub(super) tl: bool,
}

pub(super) struct MbEdges {
    pub(super) top: [u8; 24],
    pub(super) left: [u8; 16],
    pub(super) tl: u8,
    pub(super) ctop: [[u8; 8]; 2],
    pub(super) cleft: [[u8; 8]; 2],
    pub(super) ctl: [u8; 2],
}

#[derive(Clone)]
pub(super) struct Cand {
    pub(super) code: MbCode,
    pub(super) info: MbInfo,
    pub(super) ry: [u8; 256],
    pub(super) ru: [u8; 64],
    pub(super) rv: [u8; 64],
    pub(super) cost: u64,
}

impl Cand {
    pub(super) fn new() -> Box<Cand> {
        Box::new(Cand {
            code: MbCode::default(),
            info: MbInfo::default(),
            ry: [0; 256],
            ru: [0; 64],
            rv: [0; 64],
            cost: u64::MAX,
        })
    }
}

#[inline]
fn se_bits(v: i32) -> u32 {
    let k = if v > 0 {
        2 * v as u32 - 1
    } else {
        (-2 * v) as u32
    };
    2 * (31 - (k + 1).leading_zeros()) + 1
}

#[inline]
pub(super) fn mv_bits(mv: Mv, mvp: Mv) -> u32 {
    se_bits(mv.x as i32 - mvp.x as i32) + se_bits(mv.y as i32 - mvp.y as i32)
}

pub(super) fn median(a: i16, b: i16, c: i16) -> i16 {
    a.max(b).min(a.min(b).max(c))
}

pub(super) fn decimate_score(levels: &[i16]) -> u32 {
    const T: [u32; 16] = [3, 2, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut score = 0;
    let mut run = 0usize;
    for &l in levels {
        if l == 0 {
            run += 1;
            continue;
        }
        if l.unsigned_abs() > 1 {
            return 64;
        }
        score += T[run.min(15)];
        run = 0;
    }
    score
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    Normal,
    Top16x8,
    Bottom16x8,
    Left8x16,
    Right8x16,
}

pub struct SliceEnc<'a, 'b> {
    pub(super) f: &'a FrameEnc<'a>,
    pub(super) band: Band<'b>,
    pub(super) mbs: &'b mut [MbInfo],
    pub(super) y0: usize,
    pub(super) y1: usize,
    pub(super) slice_no: u16,
    pub(super) last_qp: u8,
    pub(super) prev_dqp_nz: bool,
    pub(super) cabac: Option<CabacEncoder>,
    pub(super) cavlc: Option<BitWriter>,
    pub(super) skip_run: u32,
    pub(super) stats: SliceStats,
}

pub(super) struct Src {
    pub(super) y: [u8; 256],
    pub(super) u: [u8; 64],
    pub(super) v: [u8; 64],
}
