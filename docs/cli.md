# CLI & Agent Interface Reference

The `rust-motion` CLI is a first-class product surface in Rust Motion 1.0, engineered for both human developers and autonomous AI coding agents.

---

## 1. Exit Code Contract

All invocations return stable, predictable exit codes:

| Code | Constant | Meaning | Actionable Agent Response |
| :---: | :--- | :--- | :--- |
| **0** | `EXIT_SUCCESS` | Command completed successfully. | Proceed to next task. |
| **1** | `EXIT_GENERAL_ERROR` | Unhandled error during execution. | Inspect error message in log/JSON. |
| **2** | `EXIT_INVALID_ARGUMENTS`| Flag, option, or argument parse failure. | Correct command flags according to `--help`. |
| **3** | `EXIT_MISSING_DEPENDENCY`| External dependency missing (e.g. FFmpeg, Node). | Install FFmpeg or verify `$PATH`. |
| **4** | `EXIT_VALIDATION_ERROR` | Preflight schema, script, or asset validation error. | Fix asset paths or schema errors reported in JSON details. |
| **5** | `EXIT_TIMEOUT_OR_CANCELLED`| Execution exceeded `--timeout-seconds` or was cancelled. | Increase timeout or adjust complexity. |

---

## 2. Machine-Readable Mode (`--json` & `RUST_MOTION_JSON`)

To ensure AI coding agents never need to parse unstructured human logs:
- Pass the global flag `--json`.
- Or set the environment variable `RUST_MOTION_JSON=1`.

All structured outputs and error payloads conform to valid JSON schemas.

---

## 3. Subcommands

### 3.1 `doctor`
Inspects host environment, external tools (`ffmpeg`, `ffprobe`, `node`), temp directory permissions, hardware acceleration, and supported codecs.

```bash
rust-motion doctor [--json]
```

Exit code: `0` if ready, `3` if critical dependencies are missing.

---

### 3.2 `validate`
Performs zero-render preflight checks on project files, Rhai scripts, or registered compositions.

```bash
# Validate a project file
rust-motion validate --project project.rust_motion.json [--json]

# Validate a Rhai script and props file
rust-motion validate --script video.rhai --props props.json [--json]

# Validate a registered Rust composition
rust-motion validate --composition HelloWorld [--json]
```

Exit code: `0` if valid, `4` if invalid (`EXIT_VALIDATION_ERROR`).

---

### 3.3 `inspect`
Discovers system capabilities, codec support, or inspects project and media files.

```bash
# Discover supported rendering backends and capabilities
rust-motion inspect --backends [--json]

# Discover supported codecs and hardware encoder availability
rust-motion inspect --codecs [--json]

# List all registered built-in compositions
rust-motion inspect --compositions [--json]

# Inspect a .rust_motion.json project file
rust-motion inspect project.rust_motion.json [--json]

# Inspect a media file (duration, streams, dimensions, codecs)
rust-motion inspect video.mp4 [--json]
```

---

### 3.4 `render`
Renders video or still images.

```bash
rust-motion render [OPTIONS]
```

#### Key Options:
- **Composition Source**:
  - `--composition <ID>`: Name of registered Rust composition.
  - `--script <PATH>`: Path to Rhai script.
  - `--props <PATH>`: JSON props file.
- **Output**:
  - `-o, --output <PATH>`: Output media path (default: `out.mp4`).
  - `--codec <CODEC>`: `h264`, `hevc`, `vp9`, `av1`, `prores`, `gif`, `png`, `jpeg`, `webp`.
  - `--backend <BACKEND>`: `native` (default), `gpu`, `browser`.
- **Dimensions & Timing**:
  - `--width <U32>` (default: 1920)
  - `--height <U32>` (default: 1080)
  - `--fps <F64>` (default: 30.0)
  - `--duration <U32>`: Total frames (default: 150)
  - `--frame-start <U32>`: First frame (default: 0)
  - `--frame-end <U32>`: Last frame (inclusive)
  - `--frame-step <U32>`: Subsample every Nth frame (default: 1)
- **Performance & Encoding**:
  - `--concurrency <N>`: Worker thread count (default: CPU cores)
  - `--crf <U32>`: Quality CRF (default: 18)
  - `--preset <NAME>`: FFmpeg preset (default: "fast")
  - `--hw-accel <MODE>`: `auto`, `none`, `videotoolbox`, `vaapi`, `nvenc`, `qsv`, `d3d11va`
  - `--timeout-seconds <SECS>`: Abort render after timeout
  - `--profile`: Output telemetry and pipeline stage timings table
- **Security & Sandboxing**:
  - `--sandbox-root <DIR>`: Permitted asset root directory (may be repeated)
  - `--permissive`: Disable sandbox isolation

---

### 3.5 `serve`
Launches the WebSocket preview server for Studio or custom live-editing environments.

```bash
rust-motion serve --script video.rhai --port 7890
```

---

### 3.6 `probe`
Probes audio/video container metadata and streams.

```bash
rust-motion probe video.mp4 [--full]
```

---

### 3.7 `migrate`
Scaffolds Remotion TypeScript components into Rust/Rhai templates.

```bash
rust-motion migrate -i RemotionComp.tsx -t rhai -o comp.rhai
```
