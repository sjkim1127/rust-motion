//! Host-neutral project and render-job contracts.
//!
//! Dioxus, Tauri, CLI, and Python integrations should exchange these models
//! instead of depending on one another's UI state.

mod assets;
pub mod interchange;
mod jobs;
mod model;
pub mod nle;
mod persistence;
#[cfg(not(target_arch = "wasm32"))]
mod remote;
#[cfg(not(target_arch = "wasm32"))]
mod remote_net;
pub mod scopes;
#[cfg(test)]
mod tests;
mod validate;

pub use interchange::{
    export_aaf_metadata, export_cmx3600_edl, export_fcpxml, frames_to_smpte, parse_cmx3600_edl,
    parse_fcpxml, smpte_to_frames, AafMetadata, InterchangeError,
};
pub use jobs::*;
pub use model::*;
pub use nle::{
    insert_clip, overwrite_clip, ripple_delete, roll_edit, slide_clip, slip_clip, split_clip_at,
    split_clip_at_index, trim_clip, NleClip, NleError, NleTimeline, NleTrack, NleTransition,
    TrackKind, TransitionAlignment, TransitionKind, TrimEdge, TrimMode,
};
#[cfg(not(target_arch = "wasm32"))]
use remote_net::*;
pub use scopes::{
    analyze_frame_scopes, rgb_to_luma_rec709, rgb_to_ycbcr_rec709, ChannelHistogram, ScopeGrid,
    ScopeType, VideoHistogram, VideoScopesAnalysis,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;
#[cfg(not(target_arch = "wasm32"))]
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use thiserror::Error;
