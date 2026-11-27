//! Streaming native multimedia container muxers for Rust Motion.
//!
//! Currently provides [`mp4::Mp4Muxer`], an ISO/IEC 14496-12 (MP4) muxer that streams
//! sample payloads directly to seekable storage in bounded memory while supporting:
//! - multi-track video + audio interleaving with exact per-chunk byte offsets,
//! - automatic 64-bit `co64` chunk offsets and extended `mdat` boxes (>4 GiB),
//! - B-frame composition time offsets (`ctts`),
//! - ISO/IEC 23091 `colr` (`nclx`) color metadata boxes,
//! - bounded-memory in-place `+faststart` (`moov` before `mdat`).

pub mod mp4;
pub mod probe;

pub use mp4::{Mp4Muxer, Mp4MuxerOptions};
pub use probe::{probe_isobmff, probe_isobmff_path, IsobmffProbe};
