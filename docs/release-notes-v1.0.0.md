# Rust Motion 1.0.0 — Stable, Browser-Free Programmatic Video Rendering Platform

We are thrilled to announce the official **v1.0.0** major release of **Rust Motion**! 🚀

> **Rust Motion 1.0 is a stable, deterministic, browser-free programmatic video rendering platform for Rust, Python, and autonomous coding agents, with optional browser and GPU backends for explicitly documented capabilities.**

Rust Motion delivers a high-performance, lightweight alternative to browser-based video generation engines (like Remotion). By replacing heavy headless Chrome instances with a native, pure-Rust rasterization and compositing pipeline, Rust Motion reduces memory usage by **5–10x (down to 100–200MB)**, eliminates cold-start latency (< 100ms), and guarantees bit-exact deterministic rendering.

---

## 🌟 Key Highlights

### 1. ⚡️ Browser-Free Native Core & Bounded Streaming
* **Authoritative Reference Engine**: Native CPU rasterization powered by `tiny-skia` with hardware-accelerated GPU execution via `wgpu`.
* **Bounded-Memory Pipeline**: Renders frames concurrently into an ordered ring-buffer and streams directly to FFmpeg pipes, allowing 4K rendering on constrained instances without out-of-memory crashes.
* **Full Media Codec Suite**: Built-in export support for H.264, H.265 (HEVC), ProRes, VP9, AV1, animated GIF, and PNG/JPEG/WebP stills with multi-track audio mixing.

### 2. 🤖 First-Class AI Coding Agent & Automation Surface
Rust Motion is designed from the ground up for autonomous AI coding agents (Devin, Claude Code, Cursor, Copilot Workspace) and automated CI/CD runners:
* **Predictable Exit Code Contract**:
  - `0`: `EXIT_SUCCESS`
  - `1`: `EXIT_GENERAL_ERROR`
  - `2`: `EXIT_INVALID_ARGUMENTS`
  - `3`: `EXIT_MISSING_DEPENDENCY` (e.g. FFmpeg missing)
  - `4`: `EXIT_VALIDATION_ERROR` (schema or asset errors)
  - `5`: `EXIT_TIMEOUT_OR_CANCELLED`
* **Machine-Readable Mode**: Global `--json` flag and `RUST_MOTION_JSON=1` environment variable produce strictly structured JSON diagnostic payloads.
* **Preflight Subcommands**:
  - `rust-motion doctor`: Validates host environment, hardware acceleration, FFmpeg, and external codecs.
  - `rust-motion validate`: Zero-render preflight validation for project schemas, assets, and scripts.
  - `rust-motion inspect`: Reports composition metadata, dimensions, fps, duration, and frame ranges.
* **Instant Unattended Installer**:
  ```bash
  curl -fsSL https://raw.githubusercontent.com/sjkim1127/rust-motion/main/scripts/install.sh | bash
  ```
  Zero-dependency, non-interactive installation to `~/.local/bin` in 1–2 seconds.

### 3. 📐 Mathematical Parity & Conformance Guarantees
* **Renderer Conformance Suite**: Verified bit-exact evaluation invariance regardless of access pattern (sequential forward, sequential reverse, sparse seek, repeated frames, and concurrent threads).
* **Remotion Mathematical Parity**:
  - `spring()`: Full damped harmonic oscillator physics engine matching Remotion reference values to `< 1e-4` precision.
  - `interpolate()` / `interpolateColors()`: Cubic Bezier curves, easing functions, and clamp/extend extrapolation modes.
  - `paths`: Path length computation, `interpolatePath` morphing, and `evolvePath` stroke animations.
  - `noise`: Deterministic seeded Simplex, Perlin, and Fractal Brownian Motion (FBM) generators.

### 4. 📁 Versioned Project Schema (`.rust_motion.json` v1)
* **Declarative Project Format**: Language-neutral JSON representation for compositions, multi-track timelines, transition series, asset references, and temporal markers.
* **SemVer 1.x Guarantee**: Projects adhering to `.rust_motion.json` schema v1 are guaranteed to remain compatible throughout the entire 1.x lifecycle.
* **Asset Security & Verification**: Remote assets support SHA-256 integrity verification, SSRF destination protection, and bounded size limits.

### 5. 🐍 Native Python Package (`rust-motion`)
* Available immediately via PyPI:
  ```bash
  pip install rust-motion==1.0.0
  ```
* High-performance C-ABI / PyO3 bindings with NumPy array buffer exports (`comp.render_frame_to_numpy(frame)`).
* Pre-built wheels distributed for:
  - Linux `x86_64` (manylinux2014) & `aarch64` (manylinux_2_28)
  - macOS Apple Silicon (`arm64`) & Intel (`x86_64`)
  - Windows `x86_64`

### 6. 🎙️ Kinetic Captions & Subtitles
* `rust-motion-captions`: Native word-level subtitle highlight animations for TikTok / Shorts / Reels.
* Built-in support for Whisper JSON, SubRip (`.srt`), and WebVTT (`.vtt`) formats.

### 7. 🖥️ Studio GUI Editor & Multitrack Timeline
* Powered by **Tauri v2** with high-performance WebCodecs and Three.js 3D layer preview integration.
* Interactive multitrack timeline editor with real-time scrub, seek, audio retiming against named events, and live preview.

---

## 🚀 Installation & Quickstart

### AI Agents & Standalone Installer (Recommended)
```bash
curl -fsSL https://raw.githubusercontent.com/sjkim1127/rust-motion/main/scripts/install.sh | bash
```

### macOS (Homebrew Tap)
```bash
brew tap sjkim1127/tap
brew install rust-motion
```

### Python Package (PyPI)
```bash
pip install rust-motion
# or using uv:
uv tool install rust-motion
```

### Rust Cargo
```bash
# Fast pre-built binary install via cargo-binstall:
cargo binstall rust-motion-cli

# Or compile from source:
cargo install rust-motion-cli --features rhai
```

---

## 🔄 Upgrading from 0.x (Breaking Changes)

1. **Rebranding (`dioxuscut` -> `rust-motion`)**:
   - CLI executable renamed from `dioxuscut` to `rust-motion`.
   - All environment variables migrated to `RUST_MOTION_*` (e.g., `RUST_MOTION_JSON=1`).
   - Cargo crates renamed: `rust-motion-core`, `rust-motion-composition`, `rust-motion-rasterizer`, etc.
   - Python package is now `rust-motion` (`import rust_motion`).
2. **Project File Extension**:
   - Use `.rust_motion.json` schema v1 (previously `.dioxuscut.json`).
3. **Automated Remotion Migration**:
   - Transpile legacy Remotion `.tsx` files to Rhai scripts or Rust templates:
     ```bash
     rust-motion migrate -i RemotionComponent.tsx -t rhai -o component.rhai
     ```

For full upgrade instructions, see the [1.0 Migration Guide](https://github.com/sjkim1127/rust-motion/blob/main/docs/migration-1.0.md).

---

## 📦 Release Assets & Checksums

| Asset | Platform / Architecture | Description |
| :--- | :--- | :--- |
| `rust-motion-cli-linux-x86_64.tar.gz` | Linux x86_64 (glibc) | Standalone CLI binary |
| `rust-motion-cli-macos-aarch64.tar.gz` | macOS Apple Silicon (M1/M2/M3/M4) | Standalone CLI binary |
| `rust-motion-cli-macos-x86_64.tar.gz` | macOS Intel | Standalone CLI binary |
| `rust-motion-cli-windows-x86_64.zip` | Windows x86_64 (MSVC) | Standalone CLI binary |
| `SHA256SUMS` | All platforms | SHA-256 verification checksums |

SHA-256 Checksums:
```text
c5d730ccef673fb2b39666ef96621ce33e46123497b7952a6a0b9a69ef490407  rust-motion-cli-linux-x86_64.tar.gz
2ec60fd556a41b5272cce3ee5283f58546b5a195b06f7aa352bf3fbb2d3596fe  rust-motion-cli-macos-aarch64.tar.gz
795ac0aee59b8e8b858dbee8bf2fb6d0dcf813aeeeb34277c223c68e7ec837bf  rust-motion-cli-macos-x86_64.tar.gz
60a9d4895ddcb8cb4bcc86d64ea156e99d8924b22c710d0f5077227d86f78819  rust-motion-cli-windows-x86_64.zip
```

---

## 📖 Documentation
- [Quickstart Guide](https://github.com/sjkim1127/rust-motion/blob/main/docs/quickstart.md)
- [CLI & Agent Reference](https://github.com/sjkim1127/rust-motion/blob/main/docs/cli.md)
- [Python SDK Guide](https://github.com/sjkim1127/rust-motion/blob/main/docs/python.md)
- [Project Format Specification](https://github.com/sjkim1127/rust-motion/blob/main/docs/project-format.md)
- [Security & Advisories](https://github.com/sjkim1127/rust-motion/blob/main/docs/security.md)
