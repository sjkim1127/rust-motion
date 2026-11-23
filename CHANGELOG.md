# Changelog

## 1.1.0 - 2026-11-23

### Added

- Remotion-style TypeScript SDK `@rust-motion/remotion` (`rust-motion-ts`): `interpolate`, `interpolateColors`, `spring`, `Easing`, `Sequence`, `Series`, `Loop`, `Freeze`, and a Node renderer bridge (`renderMedia`, `renderStill`, `getCompositions`, `selectComposition`) that runs the `rust-motion` CLI (#130).
- Deterministic scene authoring API in `rust-motion-composition`, with authored layer order preserved.
- Native 3D scene graph with PBR material types and a CPU software fallback.
- `rust-motion studio`: a lightweight web studio with hybrid native/browser rendering, and a headless Three.js worker for 1080p full-canvas capture.
- After Effects-style animation and compositing: expression engine (`wiggle`, `loopOut`, `loopIn`, `valueAtTime`, `linear`, `lookAt`), layer parenting and null objects, track mattes, shape layer modifiers (Trim Paths, Repeater, Wiggle, Round Corners), keyframe speed/value graphs with cubic Bezier interpolation, subframe motion blur, virtual 3D camera with point/spot lights and depth of field, Optical Glow, Displacement Map, Fractal Noise, and a particle emitter (#112-#119).
- Time remapping with optical-flow frame blending, and zero-drift media time with SMPTE drop-frame timecode (#120, #121).
- Pure-Rust codecs and containers: H.264 encoder (`rust-motion-h264enc`), H.265/HEVC 10-bit decoder, APV 10-16 bit codec, CinemaDNG sequence decoder, and an MXF (OP1a / OP-Atom) demuxer and writer (#104, #105, #122-#124).
- Document and interchange formats: PSD/PSB reader and writer with adjustment layers and blend modes (`rust-motion-psd`), Affinity documents (`rust-motion-affinity`), Compound File Binary (`rust-motion-cfb`), and AAF, EDL and FCPXML timeline interchange (#97, #99, #110, #130).
- Audio DSP engine (`rust-motion-audio-dsp`): parametric EQ, compressor, limiter, reverb, and true-peak metering (#96).
- Captions: SubStation Alpha (ASS) karaoke rendering and embedded speech synthesis (#98).
- Imaging and creative tools: natural-media paint engine with tablet recording and stroke write-on, vector boolean operations and mesh warping, image filter suite, RAW development with a lens calibration database and 3D LUTs, segmentation, matting and inpainting, multi-exposure HDR merge, panorama stitching, Ken Burns slideshows, image metadata (EXIF/XMP/IPTC/CR3/GPX), and Rec.2020 HDR scenes (#100, #101, #106-#109, #125, #127-#129).
- Sandboxed WebAssembly plugin host for custom visual effects (`rust-motion-plugins`, #126).
- Model Context Protocol server for agent-driven editing (`rust-motion-mcp`, #102). Not published to crates.io in this release.
- WebAssembly browser canvas player (`rust-motion-wasm`). Not published to crates.io in this release.
- `examples/showcase.rhai`, the README banner, and `scripts/make-showcase.sh` to regenerate it.

### Changed

- The engine is fully headless: Dioxus, webview, and the Tauri studio dependencies were removed (#130, #131). The stable CLI commands are unchanged.
- Every workspace crate opts into the workspace Clippy lints.
- Emojis were removed from documentation, sample data, and CLI output.
- First crates.io release of `rust-motion-affinity`, `-algo`, `-audio-dsp`, `-cfb`, `-cms`, `-color`, `-dng`, `-doc`, `-geom`, `-h264enc`, `-inpaint`, `-lensdb`, `-paint`, `-plugins`, `-psd`, `-raster`, `-tablet`, and `-vector`. `rust-motion-apv`, `-bitstream`, `-hevc`, `-mcp`, `-mxf`, and `-wasm` are marked `publish = false` for now.
- The Studio status endpoint and the MCP `serverInfo` report the crate version instead of a hard-coded `1.0.0`.

### Fixed

- The pure-Rust encoder fallback writes a real MP4 container instead of a raw Annex-B stream, uses an exact frame-rate ratio and BT.709 limited-range colour, and maps `--preset`; the FFmpeg path now converts and tags BT.709 the same way.
- Text glyphs are drawn at their em size. Previously they were about 27% too small relative to their advances, producing wide, uneven letter spacing.
- Text inside a group follows the group's translation, scale, and rotation in the CPU backend.
- SVG path data is parsed with the full grammar (relative commands, `S`/`T`, implicit repetition, compact numbers). Malformed paths draw up to the first error and log a warning instead of disappearing.
- TypeScript `renderMedia` / `renderStill` reject when the CLI cannot run, the composition is unknown, the render fails, or no file is produced; `renderStill` renders the requested frame and `inputProps` reach the CLI.
- Fractal Noise (simplex) uses all 12 gradient directions. A copy-paste error picked the same (y, z) gradients for 8 of 12 lattice hashes, making the noise anisotropic; noise output changes as a result.
- Clippy is clean with `--all-features -D warnings`, as the release workflow requires; a wall-clock throughput test in `rust-motion-audio-dsp` now runs only in optimised builds.
- Visual comparison help output formatting.

### Security

- Bumped `rustls` and npm development dependencies.

## 1.0.0 - 2026-10-13

### Added

- Frozen 1.0 public contracts, project schema specification (`.rust_motion.json` v1), and SemVer 1.x stability guarantees (#88).
- First-class CLI and autonomous AI agent interface with stable exit codes (0-5), machine-readable `--json` output, and `doctor`, `validate`, and `inspect` subcommands (#90).
- Renderer conformance test suite verifying bit-exact deterministic frame evaluation across sequential, reverse, random-seek, repeated, and concurrent multi-threaded access (#89).
- Documented backend capabilities matrix establishing native CPU rasterizer as the authoritative reference implementation (#89).
- Security policy, untrusted input sandboxing boundaries, and dependency vulnerability reachability analysis (`docs/security.md`, #91, #32).
- Release verification automation script (`scripts/verify-release.sh`) and reproducible release engineering runbook (`docs/release-runbook.md`, #91).
- Comprehensive documentation suite including Quickstart, Hello World tutorial, CLI reference, Python guide, and 1.0 Migration Guide (#92).

## 0.3.0 - 2026-10-13

### Added

- Interactive multitrack timeline editor in Studio with clip track assignment, duration adjustments, and playback synchronization (#52).
- Named time events (`TimeEvent`, `NamedTimeEvent`) and schema expansion in composition and project schemas.
- Structured preview cache stamps (`SourceStamp`, `FileStamp`) in `rust-motion serve` to prevent cache stamp collisions (#73).
- Bounded LRU byte-budget frame cache (`FrameCacheManager`) for rendered Lottie frames (#80).
- Precise fractional frame cache keys for Lottie rendering across differing composition/animation frame rates (#78).

### Fixed

- Isolated WebSocket seek and scrub state per connected client in `rust-motion serve`, preventing multi-client cross-talk (#74, #58).
- Rejected out-of-range WebSocket seek values exceeding `u32::MAX` in `rust-motion serve` (#72).
- Supported `i32::MIN` offsets in `SeriesSequence` calculations without integer overflow (#70).
- Prevented frame index wrap in `SceneSeries` when sequence durations boundary-align (#71).
- Enforced strict half-open duration interval (`time_secs >= total_duration_secs`) for Lottie unmount boundaries (#79).
- Aligned zero-duration `SceneLoop` behavior to match Remotion semantics (#81).
- Prevented panics when initializing spring animations with non-positive or invalid mass, damping, or stiffness (#82).
- Validated ducked audio parameters immediately upon Rhai script evaluation (#83).
- Preserved frame-varying volume keyframes during CLI native scene audio track inference (#84).
- Rebased nested scene audio start times through enclosing sequence emitters (`SceneSequence`) (#85).
- Parsed media metadata using the longest known track duration (#76).
- Preferred average frame rate (`avg_frame_rate`) over nominal frame rate (`r_frame_rate`) for variable frame rate (VFR) video containers with automatic fallback (#77).
- Renderer readiness check requires explicit successful health status before initiating render jobs (#75).

### Security

- Upgraded `rustls` to `0.23.45` to resolve TLS handshake message processing vulnerability RUSTSEC-2026-0285.

### Known limitations

- Remotion 4.0.495 compatibility gaps remain tracked in issues #4 and #5.
- Rust dependency advisories involving the Tauri, GLib, and WebKit version graph remain tracked in issue #32.

## 0.2.0 - 2026-09-24

### Fixed

- Native and GPU project renders preserve timeline clips, and saved project asset paths remain portable.
- Browser timeline hooks are scoped to each clip, and overlapping clips composite at the requested frame size.
- Partial exports honor their requested frame ranges and report progress; audio volume keyframes follow composition time.
- Concurrent render jobs use isolated temporary directories.
- Remote asset downloads reject unsafe destinations, revalidate redirects, and enforce per-asset and per-project byte limits.
- Studio's Tauri webview blocks external and lookalike-origin navigation while allowing the packaged app and local development server.

### Release

- Publish `rust-motion-project` and `rust-motion-charts` with the crates.io package graph. The release workflow checks its ordered package list against Cargo metadata.
- Build and publish Python wheels and the source distribution to PyPI from the same version tag.

### Known limitations

- Remotion 4.0.495 compatibility gaps remain tracked in issues #4 and #5.
- Rust dependency advisories involving the Tauri, GLib, and WebKit version graph remain tracked in issue #32.
