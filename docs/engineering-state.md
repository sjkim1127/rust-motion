# Rust Motion — Persistent Engineering State

This document tracks verified repository audits, architecture decisions, implemented changes, test results, and next actions across autonomous engineering cycles so subsequent sessions can continue immediately without repeating discovery.

## Current Baseline & Environment

- **Base Revision**: `7d32d36` (`refactor(composition,doc): split lib.rs into module files`)
- **Latest Verified Cycle Commits**:
  - Cycle 1 (`#161`): `db0e608`
  - Cycle 2 (`#162`): `118856e`
  - Cycle 3 (`P0/P1`): `2ae5511`
  - Cycle 4 (`P2`): `e603b34`
- **Active Working Branch**: `autonomous_engineering_mission`
- **Host / Hardware**: macOS (`Darwin 25.5.0`, `arm64`), **Apple M4 Pro**
- **Toolchain**: `rustc 1.100.0 (0ef1bc386 2026-11-09)`, `cargo 1.100.0 (3acc79c0c 2026-11-05)`, `/opt/homebrew/bin/ffmpeg`
- **Active Open PRs Checked (avoiding duplication)**:
  - `#223` (`fix/preview-media-and-audit-cleanup`) — covers `#155` & `#175`
  - `#224` (`ci/free-model-autopilot`)
  - Dependabot PRs `#210`–`#215`, `#218`–`#219`

---

## Initial Repository Investigation Findings (Verified Against Code)

1. **Workspace & Dependency Graph (`#167`–`#174`)**:
   - Workspace has 39 member crates in `Cargo.toml`.
   - Root `Cargo.toml` emits 6 `unused_workspace_dependencies` warnings (`rust-motion-apv`, `rust-motion-hevc`, `rust-motion-mcp`, `rust-motion-mxf`, `rust-motion-ts`, `rust-motion-wasm`).
   - `crates/rasterizer/Cargo.toml` unconditionally depends on 14 internal crates (`color`, `psd`, `geom`, `raster`, `paint`, `tablet`, `doc`, `vector`, `affinity`, `algo`, `plugins`, `lensdb`, `dng`, `inpaint`), pulling creative-suite/plugin dependencies into minimal callers (`#169`, `#171`).
   - `wgpu 22`, `pollster 0.4`, and `lyon_tessellation` are optional under `feature = "gpu"`.
   - `tiny-skia` appears at both `0.11.4` and `0.12.0` (via `rasterlottie 0.2.2`) in `Cargo.lock` (`#173`).
2. **GPU Pipeline & CPU/GPU Boundaries (`#159`–`#166`)**:
   - Live GPU execution works on Apple M4 Pro via `wgpu` (Metal backend), with 73 existing unit tests passing in `crates/rasterizer/src/wgpu_backend/tests.rs`.
   - `wgpu_backend/submit/frame.rs` allocates per-frame dummy 1x1 textures/samplers (`dummy_image_texture`, `default_path_mask_texture`) and per-frame vertex/index/storage buffers; image/video/text-atlas textures are cached across frames.
   - `WgpuBackend::render_frame` previously coupled scene inspection, fallback heuristics, shader suffix splitting, and overlapping-layer offscreen branching directly to `wgpu` submission code without an intermediate Render IR.

---

## Cycle 1 — Issue `#161` (Committed: `db0e608`)

### Architecture Decisions
- **Backend-Independent Render IR (`crates/rasterizer/src/render_ir/`)**:
  - Unconditionally compiled (not gated on `feature = "gpu"`) with zero `wgpu` types and zero new crate dependencies.
  - Preserves 100% compatibility of public 1.x `Scene`, `SceneNode`, and `RasterizerBackend` APIs.
  - Uses `BTreeMap` / `BTreeSet` / `BinaryHeap<Reverse<PassId>>` exclusively for deterministic scheduling across runs.
  - Explicit resource descriptors (`PixelFormat`, `ColorSpace`, `AlphaMode`, `TextureUsage`, `ResourceLifetime`) with checked byte-size arithmetic (`checked_byte_size`).
  - Conservative transient liveness analysis + pool slot reuse matching `(width, height, format, sample_count)`.
- **Narrow `RenderExecutor` Contract**:
  - `RenderExecutor::capabilities(&self) -> ExecutorCapabilities`
  - `RenderExecutor::execute_plan(&self, plan: &ExecutionPlan, config: &FrameConfig) -> Result<(RgbaImage, ExecutionReport), ExecutionError>`
  - Implemented for both `CpuReferenceExecutor` (`TinySkiaBackend`) and `WgpuBackend`.
  - Rejected: wrapping `wgpu-hal` or building a general graphics HAL; rejected mutating public `Scene` IR.

### Implemented Changes
- Created `crates/rasterizer/src/render_ir/`:
  - `types.rs`: `ResourceId`, `PassId`, `PixelFormat`, `ColorSpace`, `AlphaMode`, `ColorMetadata`, `TextureUsage`, `ResourceLifetime`, `ResourceDesc`, `ClearOp`, `RectBounds`, `CapabilityFlags`, `PassOp`, `PassNode`, `RenderGraph`.
  - `validate.rs`: `GraphValidationError`, `ResourceLiveness`, `ExecutionPlan::compile`, `check_capabilities`, `dump_text`, `dump_json`.
  - `lower.rs`: `LowerOptions`, `lower_scene_to_graph`, `lower_scene_to_plan`, `required_capabilities_for_nodes`, shared `shader_inline_opacity_supported`, `gpu_normal_texture_layer_supported`, and `trailing_overlap_texture_layer`.
  - `executor.rs`: `ExecutorCapabilities`, `ExecutionReport`, `ExecutionError`, `RenderExecutor`, `CpuReferenceExecutor`.
  - `tests.rs`: 8 comprehensive unit tests (validation errors, deterministic topological scheduling, transient pool reuse, inspectable dumps, bit-exact CPU reference equivalence, explicit `PathMask` + `Copy`, capability errors, CPU island lowering).
- Created `crates/rasterizer/src/wgpu_backend/executor.rs` implementing `RenderExecutor` for `WgpuBackend` and `WgpuBackend::compile_render_plan`.
- Wired `WgpuBackend::render_frame` (`crates/rasterizer/src/wgpu_backend/mod.rs`) through `compile_render_plan` + `RenderExecutor::execute_plan`.
- Added 4 GPU Render IR tests in `crates/rasterizer/src/wgpu_backend/tests.rs` (CPU/WGPU differential execution of shared `ExecutionPlan`, 3-pass offscreen overlap composite, unsupported `Compute` capability error, `CpuIsland` telemetry).
- Deduplicated `trailing_overlap_texture_layer`, `gpu_normal_texture_layer_supported`, `shader_opacity_supported`, and `TinySkiaBackend::render_nodes_into_pixmap`; removed dead `render_interleaved_shader_scene`.

### Known Limitations
- `WgpuBackend::render_stream` and `render_stream_gpu` still use the direct ring-buffered compilation path until Cycle 2 (`#162` / `#165`), when persistent plan caching and partial CPU islands are introduced.
- `PassOp::Compute` returns an explicit `ExecutionError::UnsupportedCapability` on both `CpuReferenceExecutor` and `WgpuBackend` until `#162` implements initial GPU compute kernels (separable blur, color transform/LUT, resampling).
- `WgpuBackend::execute_plan` delegates single-pass `Raster2D` to `compile_scene_with_path_mask`; fine-grained filter/mask CPU fallback inside `Raster2D` will be partitioned into bounded `PassOp::CpuIsland` subgraphs in `#162`.

---

---

## Cycle 2 — Issue `#162` (GPU Compute Kernels, Dummy Resource Hoisting & Bounded CPU Islands)

### Implemented Changes
- **Static Dummy GPU Resource Hoisting**:
  - Hoisted persistent `dummy_image_bg` and `default_path_mask_bg` bind groups into `WgpuContext` (`crates/rasterizer/src/wgpu_backend/context.rs`) and reused them in `submit/frame.rs` and `submit/layers.rs`, eliminating per-frame 1x1 dummy texture allocations.
  - Added `premultiplied_composite_pipeline` (`fs_premultiplied_image` + `PREMULTIPLIED_ALPHA_BLENDING`) in `pipeline.rs` and `context.rs` so translucent offscreen/compute/CPU-island surfaces composite with exact Porter-Duff premultiplied SourceOver without double-premultiplication.
- **GPU Compute Kernels & CPU Reference Parity**:
  - Created `crates/rasterizer/src/wgpu_backend/compute.rs` with WGSL `cs_box_blur` (exact `u32` integer arithmetic matching CPU `box_blur`) and `cs_color_matrix` compute pipelines.
  - Added `KERNEL_SEPARABLE_BOX_BLUR_H`, `KERNEL_SEPARABLE_BOX_BLUR_V`, and `KERNEL_COLOR_MATRIX` in `render_ir/types.rs` and implemented exact reference execution on both `CpuReferenceExecutor` and `WgpuBackend::execute_general_dag`.
  - Removed 12 lines of dead/buggy sliding-window stride arithmetic at the tail of `box_blur` in `tiny_skia_backend/filters.rs`.
- **Bounded `CpuIsland` Lowering & Streaming Integration**:
  - Extended `render_ir::lower` to partition mixed scenes in painter order into GPU segments, 2-pass compute blur layers (`layer_has_only_gpu_basic_effects`), and tight bounding-box `PassOp::CpuIsland` passes (`isolatable_cpu_node_bounds`), composited with bounded `Composite { region }`.
  - Extended `ExecutionReport` and `WgpuBackend` telemetry with `cpu_island_frames`, `cpu_island_pixels`, `cpu_island_upload_bytes`, and `compute_pass_count`.
  - Wired `WgpuBackend::render_stream` to execute multi-pass `ExecutionPlan`s (releasing `resource` mutex before `execute_plan` to avoid deadlock).

---

---

## Cycle 3 — Rust Motion Media Engine P0 (FFmpeg Isolation) & P1 (Streaming Native H.264/MP4)

### Architecture Decisions
- **Backend-Independent Media Core (`crates/media-core`)**:
  - Depends only on `thiserror` and `serde` (zero dependency on `rasterizer`, `tokio`, `wgpu`, or `ffmpeg`), preventing circular dependencies across codecs, containers, rasterizers, WASM, and CLI.
  - Defines `PixelFormat`, CICP color metadata (`ColorPrimaries`, `TransferCharacteristics`, `MatrixCoefficients`, `ColorRange`, `ColorDescription`), `VideoCodecId`, `AudioCodecId`, `ContainerFormat`, `Rational` (`from_fps` recognizing NTSC fractional rates), `VideoFrame`, `EncodedPacket`, `TrackConfig`, `MediaError`, and traits (`VideoEncoder`, `ContainerMuxer`, `MediaProbe`).
- **Streaming Container Engine (`crates/media-container`)**:
  - Implemented `Mp4Muxer<W: Read + Write + Seek>` writing compressed sample payloads straight to storage (`O(N_frames)` compact metadata in memory instead of `O(total_bytes)` payload RAM).
  - Supports extended 64-bit `mdat` + automatic `co64` (>4 GiB chunk offsets, plus `force_co64` test flag), B-frame composition offsets (`ctts` v0/v1), `colr` (`nclx` BT.709 color tags), multi-track video (`avc1`/`avcC`) + audio (`mp4a`/`esds`) interleaving with contiguous chunk detection, and bounded-memory (`256 KiB` buffer) in-place `+faststart` (`moov` before `mdat`).
  - Preserved 100% public `crates/renderer/src/mp4.rs` API compatibility by delegating `renderer::mp4::mux` to `Mp4Muxer`.
- **FFmpeg Isolation & Streaming H.264 CLI Pipeline (`rasterizer` + `cli`)**:
  - Added `EncoderBackend` (`Auto | Native | Ffmpeg`) to `PipeConfig`, `--encoder <auto|native|ffmpeg>` to `rust-motion render`, and backend-neutral `render_to_video` / `render_to_video_fallible` exports.
  - Implemented `NativeH264FrameSink` (`crates/rasterizer/src/render/native_h264.rs`) reusing pre-allocated planar YUV420p buffers and streaming `rust-motion-h264enc` directly into `Mp4Muxer`.
  - When `--encoder auto` runs without FFmpeg installed (`ErrorKind::NotFound`) for H.264 without active audio mixing, rendering transparently falls back to `NativeH264FrameSink` with atomic temp-file replacement.
  - Updated `rust-motion doctor` and `rust-motion inspect --codecs` so H.264/MP4 + still formats report ready without FFmpeg (`ok = temp_writable`), treating FFmpeg/FFprobe as optional.

### Implemented Files
- New crates: `crates/media-core/` (`Cargo.toml`, `src/lib.rs`), `crates/media-container/` (`Cargo.toml`, `src/lib.rs`, `src/mp4.rs`).
- Updated: `Cargo.toml`, `Cargo.lock`, `crates/renderer/Cargo.toml`, `crates/renderer/src/mp4.rs`, `crates/rasterizer/Cargo.toml`, `crates/rasterizer/src/lib.rs`, `crates/rasterizer/src/render/{mod.rs,types.rs,pipe.rs,native_h264.rs}`, `crates/cli/src/{lib.rs,args.rs,render.rs,main.rs,doctor.rs,inspect.rs}`, plus `RenderRequest` callers in `crates/python`, `crates/ts`, and CLI integration tests.
- Tests added: `media-core` unit tests, `media-container` unit + `ffprobe`/`ffmpeg` differential conformance tests, `rasterizer` streaming native H.264 + error/cancellation tests, and `PATH=""` FFmpeg-free E2E subprocess test (`e2e_ffmpeg_free_doctor_and_mp4_render`).

---

---

## Cycle 4 — Media Engine P2 (Native ISOBMFF/MP4/MOV Metadata Prober — Remove `ffprobe` Hard Dependency)

### Implemented Changes
- **Native Bounded Container Metadata Parser (`crates/media-container/src/probe.rs`)**:
  - Implemented `IsobmffProbe` (`MediaProbe`), `probe_isobmff`, and `probe_isobmff_path`: reads only container boxes (`ftyp`, `mvhd`, `trak`, `tkhd`, `mdhd`, `hdlr`, `stsd`, `colr`, `stts`) while seeking cleanly past `mdat` payloads without reading frames into memory.
  - Enforces safety limits on untrusted inputs (`MAX_BOX_DEPTH = 16`, `MAX_BOXES = 4096`, `MAX_MOOV_BYTES = 16 MiB`, `MAX_STTS_ENTRIES = 1_000_000`) and checked offset/size arithmetic.
  - Extracts dimensions (`stsd`/`tkhd`), exact `tkhd` display matrix rotation (`0`/`90`/`180`/`270`), CICP `colr` (`nclx`) color metadata, exact rational VFR/CFR `fps` from `stts`, and audio `mp4a`/PCM channels & sample rates.
  - Added `Mpeg4` and `Unknown` variants to `VideoCodecId` and `Unknown` to `AudioCodecId` in `crates/media-core`.
- **FFprobe-Optional `rust-motion-media` & Clean CLI JSON Streams**:
  - Wired `crates/media/src/metadata.rs` (`get_video_metadata`, `get_audio_metadata`, `parse_media`) to resolve ISO-BMFF (MP4/MOV/M4A) natively first, use `hound` for WAV and `symphonia` for MP3/AAC/PCM streams, and fall back to `ffprobe` only for containers not yet natively supported.
  - Configured CLI `tracing_subscriber` in `crates/cli/src/main.rs` with `.with_writer(std::io::stderr)` so machine-readable JSON stdout (`probe`, `inspect`, `doctor`, `--json`) stays clean even when internal decoders log diagnostics.
- **Verification**:
  - Added adversarial, rotation-matrix, and `ffprobe` differential tests in `media-container/src/probe.rs`, native probing tests in `crates/media/src/metadata.rs`, and FFprobe-free `PATH=""` `probe`, `probe --full`, and `inspect` subprocess assertions in `crates/cli/tests/e2e_cli_subprocess.rs`.

---

## Cycle 5 — Media Engine P3 (Native MP4 Sample Demuxer, FFprobe-Free `video_cache` Probing, Pure-Rust HEVC Decoding)

### Implemented Changes
- **MP4/MOV Sample Demuxer (`crates/media-container/src/demux.rs`)**:
  - Added bounded ISO-BMFF sample-table demuxer (`Mp4Demuxer<R: Read + Seek>`, `DemuxedVideoTrack`, `SampleEntry`) parsing `stsd` (`avcC`/`hvcC`/`colr`), `stsz`, `stco`/`co64`, `stsc`, `stts`, `ctts` (v0/v1), `stss`, and `edts -> elst` media-time rebasing so B-frame presentation timelines align with FFmpeg/QuickTime (`min(pts) == 0`).
  - Supports keyframe-based random-access seeking (`sample_index_for_time`, `keyframe_index_before_or_at`) and bounded reusable-buffer reads (`read_sample`).
  - Changed absent-`colr` fallback from BT.709 to `ColorDescription::unspecified_limited()` in `media-core` & `media-container` so downstream color matrix inference respects VUI / resolution defaults.
- **FFprobe-Free `video_cache` Probing & Pure-Rust HEVC Decoding (`crates/rasterizer/src/video_cache.rs`)**:
  - Updated `video_cache::probe_path` to resolve MP4/MOV/M4A containers natively via `probe_isobmff_path` first (zero `ffprobe` subprocess), falling back to `ffprobe` only for non-ISOBMFF containers.
  - Added `DecoderImpl::NativeHevc(Box<NativeHevcDecoder>)` backed by `Mp4Demuxer` + `rust_motion_hevc::Decoder::with_threads(1)` (`configure_hvcc`), keyframe seeking, deterministic DPB reordering past target PTS, and `hevc_picture_to_rgba` (8/10-bit YUV420p -> RGBA8 with VUI/CICP matrix selection, BT.601 SD fallback, and `0/90/180/270` rotation).
  - Removed `unused_workspace_dependencies` warning for `rust-motion-hevc`.
- **Verification**:
  - `media-container/src/demux.rs`: `roundtrip_mux_demux_preserves_bframe_pts_dts_co64_and_sample_bytes`, `demuxer_rejects_malformed_tables_safely`, `demuxes_and_decodes_real_hevc_mp4_via_pure_rust_decoder`.
  - `rasterizer/src/video_cache.rs`: `probe_video_metadata_resolves_mp4_without_ffprobe` (`PATH=""`), `native_hevc_mp4_decodes_frames_with_seek_and_matches_ffmpeg` (sequential + backward keyframe seek parity with FFmpeg RGBA on frames 0, 2, and 5).

---

## Remaining Roadmap & Next Priorities

- **Next Immediate Priority — Cycle 6 (Media Engine P3/P4: Apple Silicon VideoToolbox Hardware Encode & Decode)**:
  - On macOS (`cfg(target_os = "macos")`), introduce `crates/media-hwaccel` using VideoToolbox (`VTCompressionSession` / `VTDecompressionSession` / `CVPixelBuffer`) to decode H.264/HEVC MP4s natively without FFmpeg in `VideoFrameCache` (completing P3 for H.264 on macOS) and stream hardware-encoded frames directly into `Mp4Muxer` (P4).
- **Subsequent Milestones**:
  - **P4 GPU/Video**: Apple Silicon native Metal `RenderExecutor` (`#163`) + GPU-resident `MTLTexture`/`IOSurface` zero-copy hardware encode (`#164`).
  - **P5**: Full independent multimedia engine (AV1, ProRes, WebM/MKV muxing, audio AAC/Opus mixing, WASM export).
  - **Parallel Platform Priorities**: `#160`/`#166` (profiling & differential test harness), `#168`/`#169`/`#174` (dependency decoupling & feature profiles), `#194`–`#205` (geometry/CAD/nodes/3D/runtime/agent platform).
