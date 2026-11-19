# @rust-motion/remotion

> Remotion-style animation helpers and a Node bridge to the native Rust Motion renderer.

`@rust-motion/remotion` offers familiar Remotion primitives (`interpolate`, `interpolateColors`, `spring`, `Easing`, `Sequence`, `Series`, `Loop`, `Freeze`, `useCurrentFrame`, `useVideoConfig`) and a renderer bridge (`renderMedia`, `renderStill`) that runs the `rust-motion` CLI. Rendering happens in the native Rust rasterizer — no headless Chrome.

---

## Read this first: what this package is (and is not)

- **Rendering uses the native composition registry.** `renderMedia` / `renderStill` call `rust-motion render --composition <id>`, so `<id>` must be a composition known to the Rust binary (`rust-motion list-compositions`). Components you register from TypeScript with `Composition({ component })` are **not evaluated** by the native renderer; trying to render one rejects with an explicit error.
- **No React / JSX runtime.** Existing Remotion components with arbitrary DOM, CSS, or React libraries do not run here.
- **Partial API surface.** Not implemented (non-exhaustive): `registerRoot`, `staticFile`, `delayRender` / `continueRender` / `cancelRender`, `prefetch`, `calculateMetadata`, `Still`, `Html5Video` / `Html5Audio`, `useCurrentScale`, and the `@remotion/*` packages (transitions, shapes, paths, noise, captions, gif, lottie, three). Some of these exist in the Rust crates but are not exposed to TypeScript. See the [compatibility matrix](../../docs/compatibility_matrix.md).
- **Failures are errors.** `renderMedia` / `renderStill` reject if the binary cannot be run, the composition is unknown, the CLI exits non-zero, or no output file is produced.

---

## Requirements

The `rust-motion` binary must be installed and on `PATH` (or point `RUST_MOTION_BIN` at it), plus FFmpeg for video encoding. Check your setup with `rust-motion doctor`.

---

## Rendering from Node.js

```typescript
import { renderMedia, renderStill, getCompositions, selectComposition } from '@rust-motion/remotion/renderer';

// Native compositions, e.g. HelloWorld, CyberpunkGrid
const comps = await getCompositions();

// Render a video
await renderMedia({
  composition: 'HelloWorld',
  outputLocation: 'out/video.mp4',
  codec: 'h264',                 // h264 | h265 | vp9 | av1 | prores | gif | png | webm
  hwAccel: 'auto',               // auto | disabled | videotoolbox | nvenc
  inputProps: { title: 'Hello' }, // written to a temporary props file for the CLI
  onProgress: ({ progress, renderedFrames, totalFrames }) => {
    console.log(`${(progress * 100).toFixed(1)}% (${renderedFrames}/${totalFrames})`);
  },
});

// Render a single frame (png | jpeg | webp)
await renderStill({
  composition: 'HelloWorld',
  output: 'out/thumb.png',
  frame: 45,
});
```

## Animation helpers

These run in plain JavaScript and follow Remotion's semantics (covered by this package's tests):

```typescript
import { interpolate, interpolateColors, spring, Easing } from '@rust-motion/remotion';

const opacity = interpolate(frame, [0, 30], [0, 1], { extrapolateRight: 'clamp' });
const scale = spring({ frame, fps: 30, config: { damping: 12, mass: 0.5, stiffness: 120 } });
const color = interpolateColors(frame, [0, 30], ['#ff0000', '#0000ff']);
```

`Composition`, `Sequence`, `Series`, `Loop`, and `Freeze` manage frame and video-config context for these helpers; they do not produce pixels by themselves.

---

## Development

```bash
node --test tests/*.test.mjs
```

The renderer tests use a fake `rust-motion` binary, so they do not need the CLI to be built.
