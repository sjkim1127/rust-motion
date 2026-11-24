//! GPU frame submission: command recording, readback and shader-layer rendering for `WgpuBackend`.

use super::*;
use crate::scene::{Color, ImageFit};
use crate::video_cache::canonical_local_path;
use wgpu::util::DeviceExt;

mod assets;
mod drain;
mod frame;
mod layers;
mod render;
mod shader_layers;

pub(crate) struct FrameSubmission<'a> {
    pub(crate) commands: &'a [DrawCommand],
    pub(crate) path_mask: Option<&'a GpuPathMask>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) sampling_fps: f64,
    pub(crate) slot: &'a GpuFrameSlot,
    pub(crate) shader_suffix: Option<&'a [SceneNode]>,
    pub(crate) readback: bool,
}

pub(crate) struct SubmittedFrame {
    pub(crate) submission_index: wgpu::SubmissionIndex,
    pub(crate) readback_rx: Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
    pub(crate) query_rx: Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
}
