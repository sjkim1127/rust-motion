# Rust Motion Native Creative Suite & Reference Compatibility Matrix

This document records the source-backed compatibility and capability inventory of **Rust Motion** across:
1. **Remotion 4.0.495 / 4.0.535** (`#4`, `#5`, `#94`)
2. **Motion Canvas, Manim Community, MoviePy, Blender VSE, and FFmpeg** (`#46`, `#49`, `#50`, `#51`)
3. **Pure-Rust Native Creative Suite Engines** (`#95` — After Effects Core, NLE/Codecs/Audio DSP, Raster/Vector/PSD/CMS/Plugins, Camera RAW/3D LUT/Optics/HDR/Panorama/Vision/MCP, and Generative AI/3D Gaussian Splatting)

---

## 1. Remotion 4.0.495 / 4.0.535 Package & Feature Matrix (`#4`, `#5`, `#94`)

| Remotion Package / Feature | Status | Native Rust Motion Path | Verification / Evidence |
| :--- | :---: | :--- | :--- |
| `remotion` (`Composition`, `Sequence`, `Series`, `Loop`, `Freeze`) | **Supported** | `crates/composition`, `crates/core` | `crates/core/tests/remotion_parity_tests.rs` |
| `spring()`, `measureSpring()`, `interpolate()`, `interpolateColors()` | **Supported** | `crates/animation` | `crates/animation/tests/spring_parity.rs` |
| `@remotion/shapes` (`Rect`, `Circle`, `Triangle`, `Star`, `Polygon`, `Pie`) | **Supported** | `crates/shapes` | `crates/shapes/tests/e2e_shapes_tier1_tier2.rs` |
| `@remotion/paths` (`parsePath`, `interpolatePath`, `evolvePath`, `getPointAtLength`) | **Supported** | `crates/paths` | `crates/paths/src/lib.rs` unit & parity tests |
| `@remotion/transitions` (`fade`, `slide`, `wipe`, `flip`, `clockWipe`, `iris`) | **Supported** | `crates/transitions` | `crates/transitions/tests/e2e_transitions_tier1_tier2.rs` |
| `@remotion/noise` (`noise2D`, `noise3D`, `noise4D`) | **Supported** | `crates/noise` | `crates/noise/tests/e2e_noise_tier1_tier2.rs` |
| `@remotion/captions` (SRT, WebVTT, ASS Karaoke, TikTok word paging, TTS) | **Supported** | `crates/captions` | `crates/captions/src/ass.rs`, `crates/captions/src/tts.rs` |
| `@remotion/gif` & `@remotion/lottie` | **Supported** | `crates/rasterizer/src/gif_cache.rs`, `lottie_cache.rs` | `crates/rasterizer/tests/lottie_and_emoji_tests.rs` |
| `@remotion/three` & Web DOM Hybrid Rendering | **Supported** | `crates/rasterizer/src/web_backend.rs`, `crates/rasterizer/src/camera3d.rs` | `docs/hybrid-rendering.md` |
| `@remotion/mcp` (Model Context Protocol Server) | **Supported** | `crates/mcp` (`rust-motion-mcp`) | `crates/mcp/src/lib.rs` (9 tools + PNG frame inspection) |
| `@remotion/motion-blur` (Subframe Shutter-Angle Motion Blur) | **Supported** | `crates/rasterizer/src/motion_blur.rs` | `crates/rasterizer/tests/visual_effects_tests.rs` |
| `@remotion/video-matting` (Green-screen-less AI Video Matting) | **Supported** | `crates/rasterizer/src/ai_vision.rs` | `crates/rasterizer/tests/ai_vision_matting_tests.rs` |
| `@remotion/lambda` (Distributed Worker Cluster Partitioning) | **Supported** | `crates/rasterizer/src/genai_orchestrator.rs` (`DistributedWorkerCluster`) | `crates/rasterizer/tests/genai_math_schedule_media_tests.rs` |

---

## 2. Reference Engine Comparison Matrix (`#46`, `#49`, `#50`, `#51`)

| Reference Project | Capability | Status | Rust Motion Implementation | Evidence |
| :--- | :--- | :---: | :--- | :--- |
| **Motion Canvas** | Named `waitUntil` Time Events & Voice-Over Retiming (`#48`) | **Supported** | `crates/project::TimeEvent`, `docs/time-events.md` | `crates/project/src/lib.rs` |
| **Manim Community** | Deterministic `SceneDirector` (`add`, `play`, `wait`, `remove`) (`#49`) | **Supported** | `crates/rasterizer/src/play_wait_schedule.rs` | `test_deterministic_play_wait_scene_director_random_frame_order` |
| **Manim Community** | Typeset Mathematical Formulas (`MathTex`) with Addressable Parts (`#50`) | **Supported** | `crates/rasterizer/src/math_typeset.rs` | `test_math_typesetting_and_addressable_formula_parts` |
| **MoviePy** | Linked Audio/Video Media Strips with Synced Trim/Retime (`#51`) | **Supported** | `crates/rasterizer/src/play_wait_schedule.rs` (`LinkedMediaStrip`) | `test_linked_audio_video_media_strip_sync_and_independence` |
| **Blender VSE** | Multi-Track NLE Timeline (Ripple, Roll, Slip, Slide, Split, Scopes) (`#47`, `#97`) | **Supported** | `crates/project/src/nle.rs`, `crates/project/src/scopes.rs` | `crates/project/src/nle.rs` tests |

---

## 3. Native Pure-Rust Creative Suite Inventory (`#95`)

1. **After Effects Motion Graphics Core (`#112`–`#120`)**:
   - Cubic Bezier value/spatial graphs, Layer Parenting & Null Objects, Track Mattes (`Alpha`, `AlphaInverted`, `Luma`, `LumaInverted`), Shape Layer Trim Paths/Repeater/Wiggle, Subframe Motion Blur, Rhai Expression Engine (`wiggle`, `loopOut`, `time`, `valueAtTime`), 3D Virtual Camera + Depth of Field + Lights + glTF/OBJ 3D Meshes, Fractal Noise / Displacement Map / Optical Glow / Particular 3D Emitter, and Optical Flow Time Remapping.
2. **NLE, Broadcast Codecs & Audio DSP (`#96`–`#98`, `#104`–`#105`, `#121`–`#124`, `#130`)**:
   - Exact Rational Flick/SMPTE Timecode, Parametric EQ / Compressor / Limiter / Freeverb / ITU-R BS.1770 LUFS, NLE Multi-Track Timeline + EDL/FCPXML/AAF/MS-CFB Interchange, Pure-Rust H.264 Encoder, SMPTE ST 377-1 MXF Demuxer/Muxer, IETF RFC 9924 APV 10–16bit HDR Codec, Pure-Rust HEVC/H.265 10-bit Decoder, and CinemaDNG Lossless JPEG + AHD/Bilinear RAW Decoder.
3. **Raster, Vector, PSD, Affinity, CMS & Wasm Plugins (`#99`, `#106`–`#107`, `#110`, `#125`–`#126`)**:
   - Lossless PSD/PSB Reader/Writer, 27 Blend Modes, ICC v2/v4 Color Management (`crates/cms`), Natural Paint Brush & Tablet Dynamics (`crates/paint`, `crates/tablet`), Vector Boolean Ops & Mesh Warp & Contour Auto-Tracing (`crates/vector`), Native Serif Affinity (`.afphoto`/`.afdesign`) Loader (`crates/affinity`), Tile-Aware Image Filter Suite (`crates/algo`), and Sandboxed Wasm Plugin Host (`crates/plugins`).
4. **Camera RAW Grading, Optics, Computational Photography & Vision (`#100`–`#102`, `#108`–`#109`, `#127`–`#129`)**:
   - 3D LUT (`.cube`/`.3dl`) Tetrahedral Interpolation, Brown-Conrady Radial/Tangential Lens Aberration & $\cos^4(\theta)$ Vignetting (`lens_correction.rs`), Film Grain & Halation (`film_emulation.rs`), PatchMatch Inpainting & Temporal Video Matting (`crates/inpaint`, `ai_vision.rs`), Multi-Exposure HDR Radiance Merge & Ward MTB Alignment (`hdr_merge.rs`), Burt-Adelson Multi-Band Laplacian Pyramid Panorama Stitching (`panorama.rs`), Resolution-Independent Ken Burns Slideshow Compositor (`slideshow.rs`), Pure-Rust EXIF/IPTC/XMP/CR3/GPX Geotagging & Multi-Cam Sync (`exif_meta.rs`), Pure-Rust YuNet ONNX Tensor Runtime & 5-Point Facial Landmarks & Auto-Reframe (`neural_face.rs`), and Procedural Infinite-Resolution Rec. 2020 HDR Landscapes (`procedural_scenes.rs`).
5. **Generative AI & Spatial 3D Orchestration (`#103`, `#111`)**:
   - Unified Generative Media Router (OpenAI Sora, Runway Gen-3 Alpha, WorldLabs 3D Gaussian Splatting, Flux/Midjourney, ElevenLabs/Cartesia), Depth-Sorted 3D Gaussian Splat Rasterizer, Lazy Timeline Lookahead Pre-fetch Scheduler, and Distributed GPU Worker Cluster (`genai_orchestrator.rs`).
