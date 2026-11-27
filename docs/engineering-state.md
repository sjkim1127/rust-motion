# Rust Motion — Persistent Engineering State

This document tracks verified repository audits, architecture decisions, implemented changes, test results, and next actions across autonomous engineering cycles so subsequent sessions can continue immediately without repeating discovery.

## Current Baseline & Environment

- **Base Revision**: `7d32d36` (`refactor(composition,doc): split lib.rs into module files`)
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

## Remaining Roadmap & Next Priorities

- **Active Immediate Directive (Cycle 3+) — Rust Motion Media Engine (FFmpeg Decoupling P0–P5)**:
  - **P0**: Isolate FFmpeg (`media-core`, `EncoderBackend`, auto fallback, `doctor` passes without FFmpeg).
  - **P1**: Standalone streaming native H.264 + MP4 output (`media-container`, `co64`, `ctts`, `colr`, multi-track, `faststart`) wired into `rasterizer` & `cli`.
  - **P2**: Native container metadata probing (eliminating `ffprobe` hard dependency).
  - **P3**: Native video decoding & keyframe seek cache.
  - **P4**: Apple Silicon native Metal executor (`#163`) + VideoToolbox IOSurface/CVPixelBuffer hardware encoder (`#164`).
  - **P5**: Full independent multimedia engine (AV1, ProRes, WebM, WASM).
- **Parallel Platform Priorities**:
  - `#160` / `#166`: Reproducible CPU/WGPU profiling harness + differential test suite.
  - `#168` / `#169` / `#174`: Isolate core scene/render contracts from creative-suite dependencies and define feature profiles.
  - `#194`–`#205`: Geometry kernel, mesh modeling, parametric CAD, procedural node graph, materials, 3D/raytracing, animation, simulation, runtime, and agent platform integration.
