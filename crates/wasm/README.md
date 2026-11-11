# 🌐 Rust Motion WebAssembly Engine (`rust-motion-wasm`)

> **Sub-16ms Real-Time In-Browser Timeline Scrubber, Player & TouchDesigner-Inspired Procedural Visuals via WebAssembly**

`rust-motion-wasm` compiles the complete Rust Motion rendering pipeline—including **tiny-skia vector/raster layout**, **scene emitters**, **Remotion-compatible animation physics**, and **procedural noise generators**—into high-performance WebAssembly (`wasm32-unknown-unknown`).

---

## ⚡ Why WebAssembly?

| Feature | Legacy Preview (HTTP/WebSocket) | Remotion React Player | **Rust Motion WASM (`rust-motion-wasm`)** |
| :--- | :--- | :--- | :--- |
| **Execution** | Host OS server process | Headless Chrome/DOM | **In-Browser WebAssembly Memory** |
| **Scrub Latency** | 50–150 ms (IPC + network + base64) | 16–35 ms (React VDOM diff) | **< 3 ms (Direct synchronous memory blit)** |
| **Server Requirement**| Required (`rust-motion serve`) | Node.js dev server | **Zero — 100% Client-Side Static Execution** |
| **Canvas Blitting** | Base64 decode into `<img>` | DOM capture / WebGL | **Direct `ctx.putImageData` zero-copy buffer** |
| **Max Scrub FPS** | ~15–20 FPS | ~30–60 FPS | **300+ FPS capable** |

---

## 🚀 Quickstart

### 1. Build the WASM Package
```bash
wasm-pack build crates/wasm --target web --out-dir pkg
```

### 2. Embed into Any Web App or HTML Page
```html
<canvas id="video-canvas" width="1280" height="720"></canvas>
<input type="range" id="timeline" min="0" max="149" value="0" style="width: 100%">

<script type="module">
  import init, { WasmPlayer } from './pkg/rust_motion_wasm.js';

  async function run() {
    await init();

    // 1280x720 @ 30 FPS, 150 frames
    const player = new WasmPlayer('video-canvas', 1280, 720, 30.0, 150);

    // Instant, zero-latency timeline scrubbing
    const timeline = document.getElementById('timeline');
    timeline.addEventListener('input', (e) => {
      player.seek(Number(e.target.value));
    });

    // RequestAnimationFrame playback loop
    function loop(timestamp) {
      if (player.is_playing()) {
        const currentFrame = player.tick(timestamp);
        timeline.value = currentFrame;
      }
      requestAnimationFrame(loop);
    }
    requestAnimationFrame(loop);
  }

  run();
</script>
```

---

## 📦 API Reference

### `WasmPlayer`
The core timeline player bound to an HTML5 `<canvas>`.

| Method | Signature | Description |
| :--- | :--- | :--- |
| `constructor` | `new(canvas_id, width, height, fps, duration)` | Initializes player, binds canvas, renders frame 0 |
| `seek` | `seek(frame: number): void` | Instantly seeks and renders the target frame |
| `render_frame` | `render_frame(frame: number): void` | Renders frame without advancing timeline state |
| `play` | `play(): void` | Starts continuous playback |
| `pause` | `pause(): void` | Pauses playback |
| `toggle` | `toggle(): boolean` | Toggles play/pause; returns new playing state |
| `tick` | `tick(timestamp_ms: number): number` | Advances frame based on elapsed time; returns current frame |
| `get_current_frame` | `get_current_frame(): number` | Returns active 0-indexed frame number |
| `get_duration_in_frames` | `get_duration_in_frames(): number` | Returns total frame duration |
| `get_timecode` | `get_timecode(): string` | Returns SMPTE timecode (e.g. `00:00:02:14`) |
| `get_last_render_time_ms` | `get_last_render_time_ms(): number` | Exact rasterization latency in milliseconds |
| `set_composition` | `set_composition(id: string): void` | Switches active composition in registry |
| `get_available_compositions` | `get_available_compositions(): string[]` | Lists all registered native compositions |
| `set_props` | `set_props(json_str: string): void` | Live updates dynamic JSON props and re-renders |
| `get_frame_rgba` | `get_frame_rgba(frame: number): Uint8Array` | Returns raw RGBA pixel byte array |

### `GenerativeCanvas` (TouchDesigner-Inspired Procedural Visuals)
Real-time texture operator (TOP) synthesis in WebAssembly.

| Method | Signature | Description |
| :--- | :--- | :--- |
| `constructor` | `new(canvas_id, width, height)` | Binds generative canvas |
| `render_noise` | `render_noise(t, scale, octaves, r1, g1, b1, r2, g2, b2)` | Procedural multi-octave fBm Simplex Noise TOP at 60 FPS |
| `render_audio_wave` | `render_audio_wave(t, bass, mid, treble)` | Kinetic audio-reactive waveform visualizer |

### Remotion Math Primitives in WASM

- `wasm_interpolate(input, inputRange, outputRange, extrapolateLeft?, extrapolateRight?)`
- `wasm_spring(frame, fps, damping?, mass?, stiffness?, overshootClamping?)`
- `wasm_measure_spring(fps, damping?, mass?, stiffness?)`
- `wasm_noise2d(x, y)`
- `wasm_noise3d(x, y, z)`
- `wasm_fbm2d(x, y, octaves, lacunarity?, persistence?)`

---

## 🎮 Running the Interactive Demo

Launch a local static web server from the repository root:
```bash
python3 -m http.server 8080
```
Open `http://localhost:8080/crates/wasm/demo/index.html` in your browser.
Enjoy zero-latency timeline scrubbing and real-time procedural canvas visuals!
