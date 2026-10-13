# Quickstart Guide

Get up and running with Rust Motion in under 5 minutes.

---

## 1. System Requirements

- **Rust**: 1.80 or newer (for building from source or Rust development).
- **FFmpeg & FFprobe**: Available on `$PATH` for video and container encoding.
- **Node.js**: (Optional) 18+ only if using the experimental `--backend browser`.

---

## 2. Installation

### Option A: Cargo CLI (Recommended for Rust & Agents)

```bash
cargo install --locked rust-motion-cli
```

Verify your installation:
```bash
rust-motion --version
```

### Option B: Python Package

```bash
pip install rust-motion
```

---

## 3. Verify Environment with `doctor`

Rust Motion includes an environment doctor to inspect dependencies and hardware encoding capabilities:

```bash
rust-motion doctor
```

For AI agents and automated scripts, pass `--json`:
```bash
rust-motion doctor --json
```

Example output:
```json
{
  "ok": true,
  "ffmpeg": {
    "available": true,
    "path": "ffmpeg",
    "version": "ffmpeg version 7.1",
    "encoders": ["libx264", "libx265", "libvpx-vp9", "prores"]
  },
  "temp_dir": {
    "path": "/tmp",
    "writable": true
  },
  "hw_accel": "videotoolbox (hardware acceleration available)",
  "supported_codecs": ["h264", "vp9", "prores", "gif", "png", "jpeg", "webp"]
}
```

---

## 4. Render Your First Video

Render the built-in `HelloWorld` composition to MP4:

```bash
rust-motion render --composition HelloWorld -o output.mp4
```

Render a high-resolution still frame to PNG:

```bash
rust-motion render --composition HelloWorld --codec png -o frame.png
```

Inspect supported backends and capabilities:

```bash
rust-motion inspect --backends
rust-motion inspect --codecs
```

---

## 5. Next Steps

- [Write your first custom composition](hello-world.md)
- [CLI Reference & Agent Interface](cli.md)
- [Python Integration Guide](python.md)
- [Backend Capability Matrix](backends.md)
- [Project Schema Specification](project-format.md)
