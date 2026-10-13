# Changelog

## 1.0.0-rc.1 - 2026-10-13

### Added

- Frozen 1.0 public contracts, project schema specification (`.dioxuscut.json` v1), and SemVer 1.x stability guarantees (#88).
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
- Structured preview cache stamps (`SourceStamp`, `FileStamp`) in `dioxuscut serve` to prevent cache stamp collisions (#73).
- Bounded LRU byte-budget frame cache (`FrameCacheManager`) for rendered Lottie frames (#80).
- Precise fractional frame cache keys for Lottie rendering across differing composition/animation frame rates (#78).

### Fixed

- Isolated WebSocket seek and scrub state per connected client in `dioxuscut serve`, preventing multi-client cross-talk (#74, #58).
- Rejected out-of-range WebSocket seek values exceeding `u32::MAX` in `dioxuscut serve` (#72).
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

- Publish `dioxuscut-project` and `dioxuscut-charts` with the crates.io package graph. The release workflow checks its ordered package list against Cargo metadata.
- Build and publish Python wheels and the source distribution to PyPI from the same version tag.

### Known limitations

- Remotion 4.0.495 compatibility gaps remain tracked in issues #4 and #5.
- Rust dependency advisories involving the Tauri, GLib, and WebKit version graph remain tracked in issue #32.
