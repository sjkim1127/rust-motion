# Backend Capabilities and Compatibility Matrix

Rust Motion provides a multi-backend rendering architecture designed around deterministic evaluation, explicit tier contracts, and zero hidden fallbacks.

## 1. Backend Tiers

| Backend | Tier | Release Status | Determinism Guarantee | Primary Use Case |
| :--- | :--- | :--- | :--- | :--- |
| **`native`** | **Reference (Tier 1)** | **Supported (Production Ready)** | **100% Deterministic (Bit-exact CPU)** | Default CLI & server renders, CI/CD, headless automated pipelines |
| **`browser`** | **Compatibility (Tier 2)** | **Supported with Fallback** | Browser engine dependent | Complex Web/DOM animations, Remotion parity, Three.js/WebGL exports |
| **`auto`** | **Hybrid Orchestrator** | **Supported (Production Ready)** | Dynamic by sub-scene | Automatic routing: Native for motion graphics/subtitles (10x faster), Chromium for Web/3D |
| **`gpu`** | **Accelerated (Tier 2)** | **Experimental** | Hardware- & driver-dependent | Real-time interactive previews, high-throughput batch encoding |


---

## 2. Capability Matrix

| Feature / Primitive | `native` (CPU) | `gpu` (WGPU) | `browser` (Node/Chromium) | Notes |
| :--- | :---: | :---: | :---: | :--- |
| **Rectangles & Rounded Borders** | ✅ Supported | ✅ Supported | ✅ Supported | Native uses tiny-skia; GPU uses WGSL shaders |
| **Circles, Ellipses & Polygons** | ✅ Supported | ✅ Supported | ✅ Supported | Vector rasterization |
| **SVG Paths & Bezier Curves** | ✅ Supported | ✅ Supported | ✅ Supported | Native path flattening |
| **Linear & Radial Gradients** | ✅ Supported | ✅ Supported | ✅ Supported | Normalized UV space |
| **Text Layout & Shaping** | ✅ Supported | ⚠️ Via Glyph Atlas | ✅ Supported | Native uses Cosmyic/Fontdue; GPU uses texture atlas |
| **Image Decoding (PNG, JPG, WebP)**| ✅ Supported | ✅ Supported | ✅ Supported | Native image cache; GPU texture cache |
| **Video Decoding (MP4, WebM)** | ✅ Supported | ✅ Supported | ✅ Supported | Demuxed via FFmpeg pipe |
| **Lottie Vector Animations** | ✅ Supported | ⚠️ CPU Pre-raster | ✅ Supported | Headless vector rasterizer |
| **GIF Animations** | ✅ Supported | ✅ Supported | ✅ Supported | Frame-indexed playback |
| **Audio Keyframes & Mixing** | ✅ Supported | N/A (Audio passed to FFmpeg) | ✅ Supported | Audio timeline inference |
| **Filter Effects (Blur, Opacity)** | ✅ Supported | ✅ Supported | ✅ Supported | Native gaussian kernel; GPU post-pass |
| **Transforms (Rotate, Scale, 3D)** | ✅ Supported | ✅ Supported | ✅ Supported | 4x4 affine transform matrix |

---

## 3. Detailed Backend Specifications

### 3.1 `native` — The 1.0 Reference Backend

- **Engine**: Pure Rust CPU rasterizer powered by `tiny-skia`, `cosmic-text`, and software encoders.
- **Determinism**: Fully deterministic across sequential, reverse, random-seek, repeated, and concurrent rendering. The same logical frame index always produces identical pixel output.
- **Dependencies**: FFmpeg on `$PATH` for container encoding/audio muxing. Zero GPU or display server required (runs seamlessly in headless Docker containers, GitHub Actions, AWS Lambda).
- **Concurrency**: Parallel worker pool with work-stealing frame dispatch.
- **Recommended for**: All production programmatic video generation workflows.

### 3.2 `gpu` — Accelerated WGPU Backend

- **Engine**: Hardware-accelerated rendering using `wgpu` (Metal on macOS, Vulkan/VAAPI on Linux, DirectX 12 on Windows).
- **Architecture**:
  - Double-buffered command submission and readback pipeline.
  - Dedicated GPU texture cache for video frames, static images, and text glyph atlases.
  - Asynchronous CPU residual fence synchronization.
- **Fallback Policy**: When GPU hardware initialization fails or unsupported primitives are encountered, the renderer falls back to CPU rasterization with explicit telemetry logging.
- **Driver Caveats**: Driver-level floating point rounding differences may cause sub-pixel differences between different GPU hardware vendors (e.g. Apple Silicon vs NVIDIA vs AMD).

### 3.3 `browser` — Headless Browser & WebCodecs

- **Engine**: Headless Node.js process orchestrating Chromium workers via WebSocket / IPC.
- **Protocol**: Frame buffer transfer via shared memory or base64 WebCodecs packets.
- **Fallback Policy**: If Node.js or Chromium is missing from the environment, the CLI exits with exit code `3` (`EXIT_MISSING_DEPENDENCY`) or falls back to native when requested with `--fallback`.
- **Use Case**: Rendering web-first Remotion compositions or DOM-heavy layouts.

---

## 4. Conformance & Determinism Rules

1. **Frame Invariance**: Calling `render(frame_n)` must return the exact same image whether it was preceded by `frame_n - 1`, `frame_n + 10`, or no prior frames.
2. **State Isolation**: Stateful elements (such as particle emitters, physics engines, or springs) must be pure functions of the timestamp / frame number, never maintaining hidden mutable accumulators across render calls.
3. **Graceful Fallback**: Unsupported backend operations must never panic; they must record a structured fallback event and execute the reference fallback pipeline.
