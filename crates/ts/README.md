# @rust-motion/remotion

> High-performance pure Rust video engine with 1:1 Remotion TypeScript API compatibility.

`@rust-motion/remotion` allows you to write video compositions using familiar Remotion primitives and hooks (`interpolate`, `spring`, `Sequence`, `Composition`, `useCurrentFrame`, `useVideoConfig`, `renderMedia`), while executing on `rust-motion`'s ultra-fast native Rust rasterizer, audio mixer, and hardware-accelerated encoders without Headless Chrome overhead.

---

## Features

- **1:1 Remotion Compatibility**: Drop-in replacements for `interpolate()`, `spring()`, `Easing`, `<Composition>`, `<Sequence>`, `<Series>`, `<Loop>`, `<Freeze>`, and `<AbsoluteFill>`.
- **Zero Headless Chrome Overhead**: Native rendering avoids browser spawn latencies and FFmpeg pipe bottlenecks.
- **Hardware Acceleration**: VideoToolbox (Apple Silicon), NVENC (NVIDIA), or pure multi-threaded CPU SIMD rasterization.
- **Pure Rust Core**: Backed by `rust-motion-ts` and `rust-motion-cli`.

---

## Quick Start

```typescript
import {
  Composition,
  Sequence,
  useCurrentFrame,
  useVideoConfig,
  interpolate,
  spring,
  AbsoluteFill,
} from '@rust-motion/remotion';

export const MyVideo = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const opacity = interpolate(frame, [0, 30], [0, 1], {
    extrapolateRight: 'clamp',
  });

  const scale = spring({
    frame,
    fps,
    config: { damping: 12, mass: 0.5, stiffness: 120 },
  });

  return {
    style: {
      ...AbsoluteFill,
      opacity,
      transform: `scale(${scale})`,
    },
  };
};

// Register composition
Composition({
  id: 'IntroVideo',
  component: MyVideo,
  durationInFrames: 180,
  fps: 30,
  width: 1920,
  height: 1080,
});
```

### Rendering via Node.js

```typescript
import { renderMedia, renderStill } from '@rust-motion/remotion/renderer';

// Render full video
await renderMedia({
  composition: 'IntroVideo',
  outputLocation: 'out/video.mp4',
  codec: 'h264',
  hwAccel: 'auto',
  onProgress: ({ progress, renderedFrames, totalFrames }) => {
    console.log(`Render progress: ${(progress * 100).toFixed(1)}% (${renderedFrames}/${totalFrames})`);
  },
});

// Render single frame still
await renderStill({
  composition: 'IntroVideo',
  output: 'out/thumb.png',
  frame: 45,
});
```
