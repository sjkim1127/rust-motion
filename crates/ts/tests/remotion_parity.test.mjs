import test from 'node:test';
import assert from 'node:assert/strict';
import {
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
  Loop,
  Freeze,
  useCurrentFrame,
  useVideoConfig,
  getInputProps,
  pushVideoContext,
  popVideoContext,
} from '../dist/index.mjs';

test('interpolate: linear, clamping and easing', () => {
  // Linear
  const mid = interpolate(15, [0, 30], [0, 100]);
  assert.equal(mid, 50);

  // Clamping
  const under = interpolate(-10, [0, 30], [0, 100], { extrapolateLeft: 'clamp' });
  assert.equal(under, 0);

  const over = interpolate(40, [0, 30], [0, 100], { extrapolateRight: 'clamp' });
  assert.equal(over, 100);

  // Identity
  const identity = interpolate(-10, [0, 30], [0, 100], { extrapolateLeft: 'identity' });
  assert.equal(identity, -10);

  // Easing.quad
  const eased = interpolate(15, [0, 30], [0, 100], { easing: Easing.quad });
  assert.equal(eased, 25); // (0.5)^2 * 100 = 25
});

test('interpolateColors: hex and rgba transitions', () => {
  const mid = interpolateColors(0.5, [0, 1], ['#000000', '#ffffff']);
  assert.match(mid, /rgb\(128,\s*128,\s*128\)/);

  const rgba = interpolateColors(0.5, [0, 1], ['rgba(0, 0, 0, 0)', 'rgba(255, 255, 255, 1)']);
  assert.match(rgba, /rgba\(128,\s*128,\s*128,\s*0\.5\)/);
});

test('spring: physics simulation matching Remotion', () => {
  const atZero = spring({ frame: 0, fps: 30, from: 0, to: 1 });
  assert.equal(atZero, 0);

  const atTen = spring({ frame: 10, fps: 30, from: 0, to: 1 });
  assert.ok(atTen > 0.5 && atTen < 1.3);

  const clamped = spring({
    frame: 10,
    fps: 30,
    from: 0,
    to: 1,
    config: { overshootClamping: true },
  });
  assert.ok(clamped <= 1.0);

  const delayed = spring({ frame: 5, fps: 30, from: 0, to: 1, delay: 10 });
  assert.equal(delayed, 0);

  const duration = measureSpring({ fps: 30, config: { damping: 10, mass: 1, stiffness: 100 } });
  assert.ok(duration > 10 && duration < 60);
});

test('random: deterministic pseudo-random generator', () => {
  const r1 = random('my-seed');
  const r2 = random('my-seed');
  const r3 = random('another-seed');

  assert.equal(r1, r2);
  assert.notEqual(r1, r3);
  assert.ok(r1 >= 0 && r1 < 1);
});

test('Composition & Hooks: registration, Sequence and context scopes', () => {
  Composition({
    id: 'TestScene',
    component: () => 'rendered',
    durationInFrames: 120,
    fps: 60,
    width: 3840,
    height: 2160,
    defaultProps: { title: 'Rust Motion' },
  });

  const comps = getRegisteredCompositions();
  assert.ok(comps.some((c) => c.id === 'TestScene'));
  const found = getComposition('TestScene');
  assert.equal(found?.durationInFrames, 120);
  assert.equal(found?.width, 3840);

  // Push context and verify hooks
  pushVideoContext({
    id: 'TestScene',
    width: 3840,
    height: 2160,
    fps: 60,
    durationInFrames: 120,
    frame: 45,
    props: { title: 'Rust Motion' },
  });

  assert.equal(useCurrentFrame(), 45);
  assert.equal(useVideoConfig().width, 3840);
  assert.equal(getInputProps().title, 'Rust Motion');

  // Nested Sequence
  Sequence({
    from: 30,
    durationInFrames: 30,
    children: ({ frame }) => {
      assert.equal(frame, 15);
      assert.equal(useCurrentFrame(), 15);
      assert.equal(useVideoConfig().durationInFrames, 30);
    },
  });

  // Nested Loop
  Loop({
    durationInFrames: 20,
    children: ({ frame, loopIndex }) => {
      assert.equal(frame, 5); // 45 % 20 = 5
      assert.equal(loopIndex, 2);
      assert.equal(useCurrentFrame(), 5);
    },
  });

  // Nested Freeze
  Freeze({
    frame: 10,
    children: ({ frame }) => {
      assert.equal(frame, 10);
      assert.equal(useCurrentFrame(), 10);
    },
  });

  popVideoContext();
});
