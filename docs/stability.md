# Rust Motion 1.x Stability & Compatibility Policy

## 1. Product Promise

> **Rust Motion 1.0 is a stable, deterministic, browser-free programmatic video rendering platform for Rust, Python, and coding agents, with optional browser and GPU backends for explicitly documented capabilities.**

Throughout the `1.x` release series, Rust Motion guarantees semantic versioning (SemVer 2.0.0) compliance for external consumers building projects against the public Rust API, Python bindings, CLI, and project file schemas.

---

## 2. API Surface Classification

Rust Motion explicitly delineates between **Stable Public APIs**, **Experimental Capabilities**, and **Internal Implementation Details**.

### 2.1 Stable Public Surfaces (Guaranteed throughout 1.x)

Changes to these surfaces will be strictly backward-compatible across minor and patch releases (`1.x.y`):

| Surface | Modules / Crates | Scope |
|---|---|---|
| **Core Types** | `rust-motion-core` | `Scene`, `SceneNode`, `SceneFilter`, `Transform2D`, `Color`, composition contexts, and lifecycle hooks (`use_current_frame`, `use_video_config`). |
| **Composition** | `rust-motion-composition` | Composition emitters (`SceneEmitter`), temporal containers (`SceneSequence`, `SceneSeries`, `SceneLoop`, `SceneTransitionSeries`), and named markers (`TimeEvent`, `NamedTimeEvent`). |
| **Media Emitters** | `rust-motion-media` | Native media nodes (`SceneVideo`, `SceneAudio`, `SceneImage`, `SceneGif`), and media metadata probing. |
| **Project Schema** | `rust-motion-project` | `Project`, `ProjectSettings`, `Track`, `Clip`, `Asset`, `NamedTimeEvent`, project file loading, editing, saving, and local asset resolution. |
| **Renderer** | `rust-motion-renderer` | Headless render invocation, render options, frame progress callbacks, and cancellation signals. |
| **CLI** | `rust-motion-cli` (`rust-motion`) | Stable commands: `render`, `render-project`, `validate`, `inspect`, `doctor`, `probe`, `serve`. Stable JSON modes and exit codes. |
| **Python Bindings** | `rust-motion` (PyPI) | Python module exports, composition contexts, project rendering functions, and CLI invocation. |

### 2.2 Experimental Capabilities

Features marked experimental may evolve across minor releases based on community feedback. They require explicit flags or module access:

- **GPU Renderer (`wgpu_backend`)**: Hardware-accelerated texture rasterization and pipeline passes are operational but subject to driver variation. The native CPU rasterizer remains the reference implementation for 1.x.
- **Browser Worker Transport (`web_backend`)**: Headless WebCodecs execution via external browser workers.
- **Remotion Transpiler (`rust-motion migrate`)**: Heuristic migration scaffolding for Remotion `.tsx` files.
- **Experimental CLI flags**: Any argument prefixed or documented with `[EXPERIMENTAL]`.

### 2.3 Internal Implementation Details

The following are private and may change without notice:
- Internal memory cache key schemas and internal layout representations.
- Transient temporary directories and inter-process pipes.
- Private helper functions not exported at crate roots.

---

## 3. Deprecation Policy

Before any stable API is removed or replaced in future major versions:
1. The API will be marked `#[deprecated]` with a clear deprecation message and migration pointer in at least one minor release prior to removal.
2. The alternative API will be available in the same release.
3. No stable API will be removed during the `1.x` lifecycle.

---

## 4. Supported Platforms & Toolchains

### 4.1 Minimum Supported Rust Version (MSRV)
- **Rust 1.80.0** or newer.
- MSRV bumps will only occur in minor version releases (`1.x.0`) with clear release notes.

### 4.2 Python Compatibility
- Supported Python versions: **Python 3.10, 3.11, 3.12, 3.13**.
- Both source distribution (`sdist`) and pre-compiled wheels for supported platforms.

### 4.3 Operating Systems & Architectures
- **Linux**: `x86_64-unknown-linux-gnu` (glibc 2.17+ / manylinux2014), `aarch64-unknown-linux-gnu`.
- **macOS**: `x86_64-apple-darwin` (macOS 11+), `aarch64-apple-darwin` (Apple Silicon, macOS 11+).
- **Windows**: `x86_64-pc-windows-msvc` (Windows 10+).

### 4.4 External Runtime Dependencies
- **FFmpeg 4.4+** or newer (with `ffprobe`) is required for muxing video containers (`.mp4`, `.webm`, `.mov`, `.gif`) and extracting audio/video frame streams. Pure still-image rendering (`.png`, `.jpg`, `.webp`) does not require FFmpeg.
