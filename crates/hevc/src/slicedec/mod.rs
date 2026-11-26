//! Slice segment data decoding (7.3.8) with reconstruction: CTU / coding quadtree / coding unit /
//! prediction unit / transform tree parsing (CABAC, 9.3) interleaved with intra prediction, motion
//! compensation, scaling and inverse transforms. In-loop filters and row publication live in
//! `filter.rs`.
//!
//! Split by concern: data types in `types`, then the `SliceDecoder` implementation spread over
//! `ctu`, `unit`, `residual` and `prediction`.

use crate::cabac::{init_contexts, Cabac, Contexts};
use crate::error::{ensure, invalid, Result};
use crate::inter;
use crate::intra::{self, Refs};
use crate::params::{Layout, Pps, Sps};
use crate::picture::{FrameRef, RefPic, Sample};
use crate::slice::{SliceHeader, SliceType};
use crate::spec_tables::*;
use crate::tables::{qpc_420, scan_order, CTX_IDX_MAP, LEVEL_SCALE, SCAN_4};
use crate::transform;
use std::sync::Arc;

mod ctu;
mod prediction;
mod residual;
mod types;
mod unit;

pub use types::*;
