//! Slice encoding: macroblock analysis, mode decision, residual coding, reconstruction and entropy coding.
//!
//! Split by concern: data types in `types`, then the `SliceEnc` implementation spread over
//! `mb`, `residual`, `intra`, `inter` and `bslice`.

use crate::bitstream::BitWriter;

use crate::cabac::{BinSink, CabacEncoder, CabacEstimator, Contexts, CostTable};
use crate::dsp::{sad, sad16, satd, ssd};
use crate::intra::{
    filter8, mode_available, pred16x16, pred4x4, pred8x8, pred_chroma, Edge4, Edge8,
};
use crate::mbinfo::{MbInfo, MbKind, Mv, Part};
use crate::nal::{nal, SliceHeader, SliceType};
use crate::picture::{avg_into, Frame, RefPic, CHROMA_PAD, LUMA_PAD};
use crate::syntax::{MbCode, NbCtx};
use crate::tables::{chroma_qp, BLK4_XY, NUM_CTX, RASTER_TO_BLK4, ZIGZAG4, ZIGZAG8};
use crate::transform::{
    dequant4, dequant8, dequant_chroma_dc, dequant_luma_dc, fdct4, fdct8, idct4, idct8, quant4,
    quant8, quant_chroma_dc, quant_luma_dc, Deadzone, QuantTables,
};
use crate::{cabac_mb, cavlc};

mod bslice;
mod inter;
mod intra;
mod mb;
mod residual;
mod types;

pub use types::*;
