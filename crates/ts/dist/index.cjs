/**
 * @rust-motion/remotion — CommonJS Build
 */

const contextStack = [];

function pushVideoContext(ctx) {
  contextStack.push(ctx);
}

function popVideoContext() {
  return contextStack.pop();
}

function useCurrentFrame() {
  if (contextStack.length === 0) return 0;
  return contextStack[contextStack.length - 1].frame ?? 0;
}

function useVideoConfig() {
  if (contextStack.length === 0) {
    return {
      width: 1920,
      height: 1080,
      fps: 30,
      durationInFrames: 180,
      id: 'default',
    };
  }
  const top = contextStack[contextStack.length - 1];
  return {
    width: top.width ?? 1920,
    height: top.height ?? 1080,
    fps: top.fps ?? 30,
    durationInFrames: top.durationInFrames ?? 180,
    id: top.id ?? 'composition',
  };
}

function getInputProps() {
  if (contextStack.length === 0) return {};
  return contextStack[contextStack.length - 1].props ?? {};
}

function solveCubicBezier(x1, y1, x2, y2) {
  return (t) => {
    if (t <= 0) return 0;
    if (t >= 1) return 1;
    let tGuess = t;
    for (let i = 0; i < 8; i++) {
      const currentX = 3 * (1 - tGuess) * (1 - tGuess) * tGuess * x1 +
                       3 * (1 - tGuess) * tGuess * tGuess * x2 +
                       tGuess * tGuess * tGuess;
      const slope = 3 * (1 - tGuess) * (1 - tGuess) * x1 +
                    6 * (1 - tGuess) * tGuess * (x2 - x1) +
                    3 * tGuess * tGuess * (1 - x2);
      if (Math.abs(slope) < 1e-6) break;
      tGuess -= (currentX - t) / slope;
      tGuess = Math.max(0, Math.min(1, tGuess));
    }
    return 3 * (1 - tGuess) * (1 - tGuess) * tGuess * y1 +
           3 * (1 - tGuess) * tGuess * tGuess * y2 +
           tGuess * tGuess * tGuess;
  };
}

const Easing = {
  linear: (t) => t,
  ease: solveCubicBezier(0.25, 0.1, 0.25, 1.0),
  quad: (t) => t * t,
  cubic: (t) => t * t * t,
  poly: (n) => (t) => Math.pow(t, n),
  sin: (t) => 1 - Math.cos((t * Math.PI) / 2),
  circle: (t) => 1 - Math.sqrt(1 - t * t),
  exp: (t) => Math.pow(2, 10 * (t - 1)),
  elastic: (bounciness = 1) => (t) => {
    const p = 1 - bounciness * 0.7;
    return Math.pow(2, -10 * t) * Math.sin(((t - p / 4) * (2 * Math.PI)) / p) + 1;
  },
  back: (s = 1.70158) => (t) => t * t * ((s + 1) * t - s),
  bounce: (t) => {
    if (t < 1 / 2.75) {
      return 7.5625 * t * t;
    } else if (t < 2 / 2.75) {
      const t2 = t - 1.5 / 2.75;
      return 7.5625 * t2 * t2 + 0.75;
    } else if (t < 2.5 / 2.75) {
      const t2 = t - 2.25 / 2.75;
      return 7.5625 * t2 * t2 + 0.9375;
    } else {
      const t2 = t - 2.625 / 2.75;
      return 7.5625 * t2 * t2 + 0.984375;
    }
  },
  bezier: (x1, y1, x2, y2) => solveCubicBezier(x1, y1, x2, y2),
  in: (easing) => (t) => easing(t),
  out: (easing) => (t) => 1 - easing(1 - t),
  inOut: (easing) => (t) =>
    t < 0.5 ? 0.5 * easing(t * 2) : 0.5 * (2 - easing(2 - t * 2)),
};

function interpolate(input, inputRange, outputRange, options = {}) {
  if (!Array.isArray(inputRange) || !Array.isArray(outputRange)) {
    throw new Error('interpolate: inputRange and outputRange must be arrays');
  }
  if (inputRange.length !== outputRange.length || inputRange.length < 2) {
    throw new Error('interpolate: inputRange and outputRange must have the same length >= 2');
  }
  const {
    extrapolateLeft = 'extend',
    extrapolateRight = 'extend',
    easing = (t) => t,
  } = options;
  const n = inputRange.length;
  const inMin = inputRange[0];
  const inMax = inputRange[n - 1];

  if (input < inMin) {
    if (extrapolateLeft === 'identity') return input;
    if (extrapolateLeft === 'clamp') return outputRange[0];
  }
  if (input > inMax) {
    if (extrapolateRight === 'identity') return input;
    if (extrapolateRight === 'clamp') return outputRange[n - 1];
  }

  let seg = 0;
  if (input <= inputRange[0]) {
    seg = 0;
  } else if (input >= inputRange[n - 1]) {
    seg = n - 2;
  } else {
    for (let i = 0; i < n - 1; i++) {
      if (input >= inputRange[i] && input <= inputRange[i + 1]) {
        seg = i;
        break;
      }
    }
  }

  const inLo = inputRange[seg];
  const inHi = inputRange[seg + 1];
  const outLo = outputRange[seg];
  const outHi = outputRange[seg + 1];

  const span = inHi - inLo;
  const progress = span === 0 ? 0 : (input - inLo) / span;
  const eased = easing(progress);
  return outLo + (outHi - outLo) * eased;
}

function parseColor(str) {
  const s = String(str).trim();
  if (s.startsWith('#')) {
    const h = s.slice(1);
    if (h.length === 3) {
      return [
        parseInt(h[0] + h[0], 16),
        parseInt(h[1] + h[1], 16),
        parseInt(h[2] + h[2], 16),
        1,
      ];
    }
    if (h.length === 6) {
      return [
        parseInt(h.slice(0, 2), 16),
        parseInt(h.slice(2, 4), 16),
        parseInt(h.slice(4, 6), 16),
        1,
      ];
    }
    if (h.length === 8) {
      return [
        parseInt(h.slice(0, 2), 16),
        parseInt(h.slice(2, 4), 16),
        parseInt(h.slice(4, 6), 16),
        parseInt(h.slice(6, 8), 16) / 255,
      ];
    }
  }
  const match = s.match(/rgba?\s*\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)(?:\s*,\s*([\d.]+))?\s*\)/);
  if (match) {
    return [
      parseFloat(match[1]),
      parseFloat(match[2]),
      parseFloat(match[3]),
      match[4] !== undefined ? parseFloat(match[4]) : 1,
    ];
  }
  return [0, 0, 0, 1];
}

function interpolateColors(input, inputRange, outputRange) {
  if (inputRange.length !== outputRange.length || inputRange.length < 2) {
    throw new Error('interpolateColors: input and output ranges must match and have length >= 2');
  }
  const colors = outputRange.map(parseColor);
  const r = interpolate(input, inputRange, colors.map((c) => c[0]), { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' });
  const g = interpolate(input, inputRange, colors.map((c) => c[1]), { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' });
  const b = interpolate(input, inputRange, colors.map((c) => c[2]), { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' });
  const a = interpolate(input, inputRange, colors.map((c) => c[3]), { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' });

  if (a >= 0.999) {
    return `rgb(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)})`;
  }
  return `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${Number(a.toFixed(3))})`;
}

function spring(options) {
  const {
    frame = 0,
    fps = 30,
    config = {},
    from = 0,
    to = 1,
    delay = 0,
    durationInFrames,
    reverse = false,
  } = options;

  const damping = config.damping ?? 10;
  const mass = config.mass ?? 1;
  const stiffness = config.stiffness ?? 100;
  const overshootClamping = config.overshootClamping ?? false;

  const delayedFrame = reverse
    ? (durationInFrames ?? 30) - frame + delay
    : frame - delay;

  if (delayedFrame <= 0) return from;
  if (durationInFrames !== undefined && delayedFrame >= durationInFrames) return to;

  const c = damping;
  const m = mass;
  const k = stiffness;
  const zeta = c / (2 * Math.sqrt(k * m));
  const omega0 = Math.sqrt(k / m);
  const t = (delayedFrame / fps);

  let position = 1;
  if (zeta < 1) {
    const omega1 = omega0 * Math.sqrt(1 - zeta * zeta);
    const envelope = Math.exp(-zeta * omega0 * t);
    position = 1 - envelope * (Math.cos(omega1 * t) + (zeta * omega0 / omega1) * Math.sin(omega1 * t));
  } else if (zeta === 1) {
    const envelope = Math.exp(-omega0 * t);
    position = 1 - envelope * (1 + omega0 * t);
  } else {
    const r1 = -omega0 * (zeta - Math.sqrt(zeta * zeta - 1));
    const r2 = -omega0 * (zeta + Math.sqrt(zeta * zeta - 1));
    position = 1 - (r2 * Math.exp(r1 * t) - r1 * Math.exp(r2 * t)) / (r2 - r1);
  }

  if (overshootClamping) {
    position = Math.max(0, Math.min(1, position));
  }
  return from + (to - from) * position;
}

function measureSpring(options) {
  const { fps = 30, config = {}, threshold = 0.005 } = options;
  for (let f = 0; f < 3000; f++) {
    const val = spring({ frame: f, fps, config, from: 0, to: 1 });
    if (Math.abs(val - 1) < threshold) {
      let settled = true;
      for (let lookahead = 1; lookahead <= 20; lookahead++) {
        const nextVal = spring({ frame: f + lookahead, fps, config, from: 0, to: 1 });
        if (Math.abs(nextVal - 1) >= threshold) {
          settled = false;
          break;
        }
      }
      if (settled) return f;
    }
  }
  return 30;
}

function random(seed) {
  let h = 0;
  if (typeof seed === 'number') {
    h = Math.floor(seed);
  } else {
    const str = String(seed);
    for (let i = 0; i < str.length; i++) {
      h = (Math.imul(31, h) + str.charCodeAt(i)) | 0;
    }
  }
  let s = h >>> 0;
  s = (s + 0x6d2b79f5) | 0;
  let t = Math.imul(s ^ (s >>> 15), 1 | s);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}

const compositionRegistry = new Map();

function Composition(props) {
  const { id, component, durationInFrames = 180, fps = 30, width = 1920, height = 1080, defaultProps = {} } = props;
  compositionRegistry.set(id, {
    id,
    component,
    durationInFrames,
    fps,
    width,
    height,
    defaultProps,
  });
  return null;
}

function getRegisteredCompositions() {
  return Array.from(compositionRegistry.values());
}

function getComposition(id) {
  return compositionRegistry.get(id);
}

function Sequence(props) {
  const { from = 0, durationInFrames, children } = props;
  const currentFrame = useCurrentFrame();
  const localFrame = currentFrame - from;

  if (localFrame < 0) return null;
  if (durationInFrames !== undefined && localFrame >= durationInFrames) return null;

  const currentConfig = useVideoConfig();
  pushVideoContext({
    ...currentConfig,
    frame: localFrame,
    durationInFrames: durationInFrames ?? currentConfig.durationInFrames - from,
  });

  const res = typeof children === 'function' ? children({ frame: localFrame }) : children;
  popVideoContext();
  return res;
}

function Series(props) {
  return props.children;
}
Series.Sequence = Sequence;

function Loop(props) {
  const { durationInFrames, times, children } = props;
  const currentFrame = useCurrentFrame();
  const localFrame = currentFrame % durationInFrames;
  const loopIndex = Math.floor(currentFrame / durationInFrames);

  if (times !== undefined && loopIndex >= times) return null;

  const currentConfig = useVideoConfig();
  pushVideoContext({
    ...currentConfig,
    frame: localFrame,
  });
  const res = typeof children === 'function' ? children({ frame: localFrame, loopIndex }) : children;
  popVideoContext();
  return res;
}

function Freeze(props) {
  const { frame = 0, children } = props;
  const currentConfig = useVideoConfig();
  pushVideoContext({
    ...currentConfig,
    frame,
  });
  const res = typeof children === 'function' ? children({ frame }) : children;
  popVideoContext();
  return res;
}

const AbsoluteFill = {
  position: 'absolute',
  top: 0,
  left: 0,
  right: 0,
  bottom: 0,
  width: '100%',
  height: '100%',
  display: 'flex',
  flexDirection: 'column',
};

function Audio(props) {
  return { type: 'audio', props };
}

function Video(props) {
  return { type: 'video', props };
}

function Img(props) {
  return { type: 'img', props };
}

function OffthreadVideo(props) {
  return { type: 'offthread-video', props };
}

module.exports = {
  useCurrentFrame,
  useVideoConfig,
  getInputProps,
  pushVideoContext,
  popVideoContext,
  interpolate,
  interpolateColors,
  Easing,
  spring,
  measureSpring,
  random,
  Composition,
  getRegisteredCompositions,
  getComposition,
  Sequence,
  Series,
  Loop,
  Freeze,
  AbsoluteFill,
  Audio,
  Video,
  Img,
  OffthreadVideo,
};
