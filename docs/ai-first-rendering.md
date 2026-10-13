# AI-first rendering contract

Rust Motion treats the project file and render-job API as the primary interface.
The desktop preview is optional; an agent can run the same workflow through the
CLI or Tauri commands.

## Project input

Projects use the versioned `.rust_motion.json` schema. A language-neutral JSON
Schema is available at `schemas/rust-motion-project-v1.schema.json`.

```json
{
  "version": 1,
  "composition": "three_preview",
  "settings": {
    "width": 1280,
    "height": 720,
    "fps": 30,
    "duration": 150,
    "scale": 1.0,
    "crf": 18,
    "preset": "fast",
    "concurrency": 4,
    "frame_step": 1,
    "backend": "browser",
    "browser_image_format": "jpeg",
    "browser_jpeg_quality": 90,
    "browser_frame_timeout_ms": 30000,
    "browser_transport_retries": 1
  },
  "props": { "color": "#6c63ff" },
  "assets": [
    { "id": "logo", "path": "assets/logo.png", "kind": "image", "sha256": "..." },
    { "id": "font", "path": "assets/Inter-Regular.woff2", "kind": "font" }
  ],
  "tracks": []
}
```

The same `Project` model is shared by Dioxus, Tauri, and CLI integrations. Tauri
honors the project backend (`native`, `gpu`, or `browser`) instead of replacing
it with a host-specific default.
Set `settings.concurrency` to a positive worker count when a project needs
reproducible parallelism; omit it for host-selected defaults.
Browser projects may also set `browser_image_format` to `png` or `jpeg`,
optionally configure JPEG quality from 1 to 100, and set a positive worker
response timeout and transport retry count. These settings are applied by
Tauri's embedded Chromium backend and validated before submission.

Discover Native composition IDs without starting a UI:

```sh
RUST_MOTION_JSON=1 rust-motion list-compositions
```

Validate a project before submitting it:

```sh
rust-motion validate-project project.rust_motion.json
```

This performs the same strict Rust validation used by Tauri, including schema
version, dimensions, timing, backend, and unknown-field checks.

Asset paths are resolved relative to the project file, not the process working
directory. Native and GPU validation also checks that local assets exist; when
`sha256` is provided, the file digest must match before rendering starts.
Remote URLs and data URIs are passed through for the Browser backend and are not
treated as local Native/GPU files.

Render the validated project directly:

```sh
RUST_MOTION_JSON=1 rust-motion render-project project.rust_motion.json --output out.mp4
```

Native/GPU hosts can opt into downloading HTTP(S) assets before rendering:

```bash
rust-motion render-project project.rust_motion.json --asset-cache-dir .rust-motion-assets
```

The materializer applies a 256 MiB per-asset limit and a configurable 1 GiB
project total by default. Use `--max-total-asset-bytes` to change the project
limit. It verifies a declared `sha256` and rewrites `asset://<id>` references
to the cached local file. Studio uses the same per-asset limit and reads its
project total from `RUST_MOTION_MAX_TOTAL_REMOTE_ASSET_BYTES` (1 GiB by default).
Without this option, remote assets remain Browser-backend URLs and are not
fetched by the CLI.

`render-project` maps the project backend to the shared renderer and emits the
same machine-readable success/error result as `render`.

Video renders may use `--frame-step N` to follow Remotion's `everyNthFrame`
behavior: source frames are sampled every N frames and the output FPS is
reduced accordingly. Still-image codecs remain single-frame operations; zero
is always rejected.

For Browser projects, asset paths and timeline clips are passed to the worker
before the first frame. Images, fonts, and video metadata are loaded once and
reused across frames; a failed load is retried according to the Browser frame
retry policy. Active clips receive their local frame and clip props.

Assets with `kind: "audio"` are also forwarded as encoder inputs by
`render-project` and the Tauri job host, so an AI-generated project can declare
audio in the shared project file without a host-specific CLI flag.
Use `kind: "lottie"` for JSON animation assets; Browser hosts preload and
reuse the parsed animation data, while Native hosts rasterize the same asset
through the shared Scene contract.
Props may refer to a declared local asset with `asset://<id>`; CLI and Tauri
resolve that URI through the manifest before rendering and restore portable
relative paths when saving the project.

## Job lifecycle

1. Submit the project with `submit_project` (when assets are already resolved),
   or use Tauri's `submit_project_from_path` to load, validate, and resolve
   relative assets against the project file before submission. Retain the
   returned job id.
2. Start a browser job with `start_render_job(id, output)`.
3. Poll `get_render_job(id)` or `list_render_jobs()`.
4. Cancel active work with `cancel_render_job(id)`; cancellation reaches the
   FFmpeg/Chromium render control.
5. Retry a failed or cancelled job with `retry_render_job(id)`.

Valid progress states are `queued`, `preparing`, `rendering`, `encoding`,
`completed`, `failed`, and `cancelled`. A completed job includes its output path.

## Browser backend

The CLI and Tauri host use a persistent worker pool. Configure the pool with:

```sh
export RUST_MOTION_BROWSER_WORKER=$PWD/apps/studio-tauri/scripts/three-render-worker.mjs
# Optional: use a bundled or platform-specific Node.js executable.
export RUST_MOTION_BROWSER_NODE=node
# Optional: load an AI-generated/browser composition module after the app loads.
export RUST_MOTION_BROWSER_COMPOSITION_MODULE=http://localhost:1420/src/compositions/demo.js
# Optional during development: use an already-running composition server.
export RUST_MOTION_BROWSER_URL=http://localhost:1420
export RUST_MOTION_BROWSER_CONCURRENCY=4
# Optional: fail readiness or an async renderFrame() that exceeds this limit.
export RUST_MOTION_BROWSER_FRAME_TIMEOUT_MS=30000
# Optional: retry a failed frame (default: 1 retry).
export RUST_MOTION_BROWSER_TRANSPORT_RETRIES=1
# Optional: lower-latency lossy browser capture for opaque video workloads.
export RUST_MOTION_BROWSER_IMAGE_FORMAT=jpeg
export RUST_MOTION_BROWSER_JPEG_QUALITY=90
# Optional: keep encoded frame bytes out of JSON/base64 transport.
export RUST_MOTION_BROWSER_TRANSPORT=file
```

Rust embedders such as Tauri can configure the same capture policy on an
individual `BrowserFrameBackend` without process-wide environment variables:

```rust,ignore
let backend = BrowserFrameBackend::with_concurrency(node, worker, url, 4)?
    .with_image_format("jpeg")
    .with_jpeg_quality(90)
    .with_transparent(false)
    .with_frame_timeout(std::time::Duration::from_secs(30))
    .with_transport_retries(2)
    .with_file_transport(true);
```

This keeps browser transport policy local to a Tauri render job while the
wire protocol remains identical for Dioxus desktop, CLI, and other hosts.
The `file` transport writes each encoded PNG/JPEG frame to a process-scoped
temporary file and deletes it after Rust decodes it; omit the option to retain
the JSON/base64 compatibility path.

On the Apple M4 Pro reference machine, a 1280x720 Three.js workload rendered
180 frames with four workers at about 89 FPS using PNG/base64, 110 FPS using
PNG/file, and 116 FPS using JPEG quality 90. Use `file` for lossless local
Tauri/CLI rendering, and use JPEG only when the composition is opaque and a
small lossy transport trade-off is acceptable. The base64 path remains the
portable default for hosts that cannot share a process-scoped file.

The packaged Tauri renderer automatically selects PNG/file transport when a
project does not specify `browser_transport`; set it to `base64` to force the
portable JSON transport.

When the packaged Tauri app runs without `RUST_MOTION_BROWSER_URL`, the host
serves its bundled `dist` directory from a dynamic loopback port and points the
Chromium workers at that server. Production browser rendering therefore does
not require a separately running Vite or Dioxus development server.

The browser host exposes `window.rust_motion.registerComposition(id, render)` and
`window.rust_motion.listCompositions()`;
Three.js or React Three Fiber adapters can register composition-specific frame
functions without changing the Rust protocol. The function receives
`{composition, frame, fps, props, assets, width, height, durationInFrames}` and
may return a Promise for asynchronous asset loading. `frame` is the local frame
when the composition is used inside a timeline clip; `width`, `height`, and
`durationInFrames` let Three.js/R3F code configure its camera and responsive
scene deterministically for both Studio preview and headless export.
When `RUST_MOTION_BROWSER_COMPOSITION_MODULE` is set, the worker also discovers
module exports automatically: an optional `register(api)` function is called,
then functions in `compositions` (or the default export object) are registered
under their object keys. This lets an AI-generated entrypoint expose multiple
Three.js scenes without a separate bootstrap file.
For adapters that expect Remotion's hook-shaped API, the same host exposes
`window.rust_motion.useCurrentFrame()` and
`window.rust_motion.useVideoConfig()`. These return the current explicit frame and
the synchronized `{fps, width, height, durationInFrames}` for the active
request; they do not introduce React or a browser render loop.
`window.rust_motion.staticFile(path)` resolves a portable asset URL against the
active Vite/Tauri origin, and `getStaticFiles()` returns the asset paths declared
on the current project render request.
`window.rust_motion.getInputProps()` returns a cloned snapshot of the active
project props, matching the non-hook Remotion API while preventing a
composition from mutating the host-owned request object.
`getRemotionEnvironment()` (and its hook-shaped alias
`useRemotionEnvironment()`) reports `{isRendering, isStudio, isPlayer}` so a
composition can distinguish headless export from interactive preview.
For non-React Three.js compositions, `window.rust_motion.useVideoTexture(src,
{frame, fps})` (also available as `getVideoTexture`) returns a cached
`THREE.VideoTexture` after `loadeddata` and
seeks it to the requested composition frame; call
`releaseVideoTexture(src)` when the composition no longer needs it. This is the
Browser host equivalent of vendor `useVideoTexture` and avoids creating a new
GPU texture on every frame.
For Remotion-style asynchronous readiness, a composition may call
`window.rust_motion.delayRender(reason)` and later
`window.rust_motion.continueRender(handle)`. Calling
`window.rust_motion.cancelRender(handle, reason)` fails the current frame and
propagates the reason to the CLI/Tauri job. A cancelled render is not captured
as a partially prepared frame.

Browser hosts also expose `window.rust_motion.registerLottieAdapter(adapter)`.
An adapter implements `render(element, state)` and may use `lottie-web` or
another renderer; `state` includes the source, frame, FPS, time, playback rate,
and loop behavior. This keeps the Rust protocol independent of a particular
JavaScript animation library while allowing Dioxus Lottie elements to render
in the Chromium compatibility backend.

`CanvasImage` follows the same boundary. Dioxus emits a stable
`data-rust-motion-canvas-image` marker with `data-src` and `data-fit`, and the
browser host draws the preloaded drawable onto the canvas for each requested
frame. Native VDOM converts the marker to a cached `SceneNode::Image` fallback;
it does not emulate arbitrary Canvas or WebGL APIs.

Native hosts also expose `rust_motion_media::get_image_dimensions()`, a bounded
header-only image inspection helper corresponding to Remotion's
`getImageDimensions()`. It returns width and height without allocating the full
decoded pixel buffer, so AI-generated compositions can validate image layout
before scheduling a render.

Browser compositions can use `window.rust_motion.getImageDimensions(src)` for the
same Promise-based query. Results are cached by source and expose
`{width, height}`, matching vendor `@remotion/media-utils` behavior.
Browser compositions can likewise call `window.rust_motion.getVideoMetadata(src)`
to receive `{durationInSeconds, width, height, aspectRatio, isRemote}` with
source-level caching.
`window.rust_motion.getAudioDurationInSeconds(src)` (also available as
`getAudioDuration`) provides the cached duration for audio-driven compositions.
Audio analysis is shared through `getAudioData(src, {sampleRate, requestInit})`
and the hook-shaped alias `useAudioData`. The host also provides
`getWindowedAudioData()`/`useWindowedAudioData()`, `getWaveformPortion()`,
`visualizeAudioWaveform()`, and `visualizeAudio()` for frame-driven waveform
and spectrum work. `audioBufferToDataUrl()` converts synthesized Web Audio to a
float32 WAV data URL, while `prefetch()` plus `usePreload()` keeps media in a
blob/base64 cache and reports streaming progress. These are framework-free
adapters: React is not required in a Three.js composition. Native Rust exposes
the corresponding `get_waveform_bars()` and `visualize_audio_waveform()` APIs
through `rust_motion_media`.

For container inspection, the browser exposes `parseMedia({src, fields})` and
`readMediaRange(source, start, endExclusive, {requestInit})`. The native side
provides `parse_media()` and `read_media_range()` with the same half-open range
contract and a 16 MiB per-read bound. Browser reads use HTTP `Range` and verify
the returned length; native reads seek locally and read exactly the requested
bytes. `parseMedia()` also accepts Remotion-shaped `onDimensions`,
`onDurationInSeconds`, and `onParseProgress` callbacks; progress is currently
metadata-level (the byte total remains unknown) until the incremental container
parser lands. WAV metadata is already parsed incrementally, and
`probeIsoBmff()` discovers MP4/MOV top-level boxes (`ftyp`, `moov`, `mdat`)
without reading payloads, while `parseIsoBmffMovieHeader()` also reads
`mvhd` and `trak/mdia/mdhd/hdlr` to report movie and track timing. These APIs
also expose bounded `stts`, `stsz`, `stco/co64`, and `stss` sample tables for
each parsed track, before sample offsets are expanded through `stsc`. This is
complemented by `ctts` composition offsets and decoder-neutral `codecConfig` extraction for `avcC`, `hvcC`,
`av1C`, and `esds`, allowing a WebCodecs adapter to construct its codec
configuration without another whole-file read. This is
the foundation for adding sample parsers without forcing a whole asset into
memory.

Studio integrations may use `watchStaticFile(fileName, callback)` and dispatch
`remotion_staticFilesChanged` with `{files: [{name, lastModified}]}`. The
returned `cancel()` handle makes the watcher safe to use during composition
hot reloads.

Each worker owns one Chromium page and serializes its own requests; the shared
streaming pipeline schedules different frames across the pool while preserving
output order. Use concurrency greater than one only after measuring the target
composition; Chromium startup and WebGL resource contention can make a larger
pool slower.

PNG remains the default and is recommended for transparent or pixel-sensitive
stills. JPEG capture reduces browser-to-Rust payload size for opaque video
frames; FFmpeg still performs the final codec encoding.

For headless automation, `rust-motion serve` exposes `GET /health` and
`GET /frame?frame=N` as JSON. The latter returns the rendered PNG as
`png_base64` together with its frame and dimensions, so an agent can inspect or
store a frame without opening the preview UI.
Tauri hosts additionally expose `list_browser_compositions`, which performs a
worker handshake and returns the registered Browser composition IDs without
requiring the Studio UI.

For render automation, set `RUST_MOTION_JSON=1`. Successful and failed `render`
commands then emit one machine-readable result line on stdout. Human tracing
remains on stderr, so an agent can parse stdout without log filtering.
`readIsoBmffSample(source, trackIndex, sampleIndex)` then fetches one encoded
sample range and returns its bytes plus offset/keyframe metadata. The payload
is intentionally decoder-neutral so Chromium WebCodecs and a future native
decoder can share the same sample scheduler.
`readIsoBmffSamples()` batches those reads with bounded concurrency (default 4)
and accepts a previously parsed structure, avoiding repeated header parsing.
`createIsoBmffEncodedChunk(sample, 'video'|'audio')` provides the final
WebCodecs `EncodedVideoChunk`/`EncodedAudioChunk` bridge when those globals are
available.
`makeIsoBmffWebCodecsConfig(track)` derives a WebCodecs codec string and
description from the parsed configuration (including H.264 `avc1` profile
bytes).
`decodeIsoBmffVideo()` supplies a bounded `VideoDecoder` lifecycle over those
samples; codec string and optional description remain explicit at the call
site so the same parser can serve different WebCodecs implementations.
`decodeIsoBmffAudio()` provides the corresponding `AudioDecoder` lifecycle and
returns owned `AudioData` objects for the shared audio pipeline.
Both decoders default to at most 256 samples per call via `maxSamples`, making
memory use bounded for long tracks; callers can choose a different positive
limit for windowed decoding.
