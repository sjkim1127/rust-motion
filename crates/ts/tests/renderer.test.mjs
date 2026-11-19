import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  getCompositions,
  selectComposition,
  renderMedia,
  renderStill,
} from '../dist/renderer.mjs';
import { Composition } from '../dist/index.mjs';

// A fake `rust-motion` binary so the bridge can be tested without building the CLI.
// FAKE_MODE: "ok" (default) writes the requested output; "fail" exits 1; "silent" exits 0
// without writing a file. Every invocation's argv is appended to FAKE_LOG as JSON lines.
const FAKE_BIN = `#!/usr/bin/env node
const fs = require('node:fs');
const args = process.argv.slice(2);
if (process.env.FAKE_LOG) fs.appendFileSync(process.env.FAKE_LOG, JSON.stringify(args) + '\\n');
if (args[0] === 'list-compositions') {
  console.log(JSON.stringify({ compositions: ['HelloWorld', 'CyberpunkGrid'], ok: true }));
  process.exit(0);
}
const mode = process.env.FAKE_MODE || 'ok';
const out = args[args.indexOf('--output') + 1];
const props = args.indexOf('--props');
if (props >= 0 && process.env.FAKE_PROPS_COPY) {
  fs.copyFileSync(args[props + 1], process.env.FAKE_PROPS_COPY);
}
if (mode === 'fail') {
  console.error("Error: Unknown composition 'X'. Available compositions: HelloWorld");
  process.exit(1);
}
process.stderr.write('[Rust Motion Progress] 1 / 4 frames (25.0%)...\\n');
process.stderr.write('[Rust Motion Progress] 4 / 4 frames (100.0%)...\\n');
if (mode !== 'silent') fs.writeFileSync(out, 'fake');
`;

function setup() {
  const dir = mkdtempSync(join(tmpdir(), 'rm-ts-test-'));
  const bin = join(dir, 'rust-motion');
  writeFileSync(bin, FAKE_BIN);
  chmodSync(bin, 0o755);
  process.env.RUST_MOTION_BIN = bin;
  process.env.FAKE_LOG = join(dir, 'log.jsonl');
  delete process.env.FAKE_MODE;
  delete process.env.FAKE_PROPS_COPY;
  const calls = () =>
    existsSync(process.env.FAKE_LOG)
      ? readFileSync(process.env.FAKE_LOG, 'utf8').trim().split('\n').map((l) => JSON.parse(l))
      : [];
  return { dir, calls, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

test('renderer: getCompositions & selectComposition use the native registry', async () => {
  const t = setup();
  try {
    const comps = await getCompositions();
    assert.deepEqual(comps.map((c) => c.id), ['HelloWorld', 'CyberpunkGrid']);

    const selected = await selectComposition({ id: 'HelloWorld' });
    assert.equal(selected.id, 'HelloWorld');
    assert.equal(selected.fps, 30);

    await assert.rejects(selectComposition({ id: 'DoesNotExist' }), /not found/);
  } finally {
    t.cleanup();
  }
});

test('renderer: renderMedia runs the CLI, reports progress, and checks the output file', async () => {
  const t = setup();
  try {
    const output = join(t.dir, 'out', 'video.mp4');
    const seen = [];
    const result = await renderMedia({
      composition: 'HelloWorld',
      outputLocation: output,
      codec: 'h264',
      onProgress: (p) => seen.push(p),
    });

    assert.equal(result.outputLocation, output);
    assert.ok(existsSync(output));
    assert.ok(seen.length >= 2);
    assert.ok(seen.every((p) => p.progress >= 0 && p.progress <= 1));
    assert.equal(seen.at(-1).progress, 1);
    // Real CLI progress ("1 / 4 frames") is parsed, not just the final callback.
    assert.ok(seen.some((p) => p.renderedFrames === 1 && p.totalFrames === 4));

    const [args] = t.calls();
    assert.equal(args[0], 'render');
    assert.equal(args[args.indexOf('--composition') + 1], 'HelloWorld');
    assert.equal(args[args.indexOf('--codec') + 1], 'h264');
  } finally {
    t.cleanup();
  }
});

test('renderer: renderMedia passes inputProps to the CLI as a props file', async () => {
  const t = setup();
  try {
    process.env.FAKE_PROPS_COPY = join(t.dir, 'props-copy.json');
    await renderMedia({
      composition: 'HelloWorld',
      outputLocation: join(t.dir, 'v.mp4'),
      inputProps: { title: 'Hi' },
    });
    assert.deepEqual(JSON.parse(readFileSync(process.env.FAKE_PROPS_COPY, 'utf8')), { title: 'Hi' });
  } finally {
    t.cleanup();
  }
});

test('renderer: renderMedia rejects when the CLI fails', async () => {
  const t = setup();
  try {
    process.env.FAKE_MODE = 'fail';
    await assert.rejects(
      renderMedia({ composition: 'Nope', outputLocation: join(t.dir, 'x.mp4') }),
      /exited with code 1[\s\S]*Unknown composition/,
    );
  } finally {
    t.cleanup();
  }
});

test('renderer: renderMedia rejects when the binary is missing', async () => {
  const t = setup();
  try {
    process.env.RUST_MOTION_BIN = join(t.dir, 'does-not-exist');
    await assert.rejects(
      renderMedia({ composition: 'HelloWorld', outputLocation: join(t.dir, 'x.mp4') }),
      /could not run the rust-motion binary/,
    );
  } finally {
    t.cleanup();
  }
});

test('renderer: renderMedia rejects when the CLI succeeds but writes no file', async () => {
  const t = setup();
  try {
    process.env.FAKE_MODE = 'silent';
    await assert.rejects(
      renderMedia({ composition: 'HelloWorld', outputLocation: join(t.dir, 'x.mp4') }),
      /produced no file/,
    );
  } finally {
    t.cleanup();
  }
});

test('renderer: a TypeScript-only composition failure explains the limitation', async () => {
  const t = setup();
  try {
    Composition({
      id: 'TsOnlyComp',
      component: () => ({}),
      durationInFrames: 10,
      fps: 30,
      width: 640,
      height: 360,
    });
    process.env.FAKE_MODE = 'fail';
    await assert.rejects(
      renderMedia({ composition: 'TsOnlyComp', outputLocation: join(t.dir, 'x.mp4') }),
      /does not evaluate TypeScript components/,
    );
  } finally {
    t.cleanup();
  }
});

test('renderer: renderStill renders a single frame through the CLI', async () => {
  const t = setup();
  try {
    const output = join(t.dir, 'thumb.png');
    const result = await renderStill({
      composition: 'HelloWorld',
      output,
      frame: 15,
      imageFormat: 'png',
    });

    assert.equal(result.output, output);
    assert.equal(result.frame, 15);
    assert.equal(result.imageFormat, 'png');
    assert.ok(existsSync(output));

    const [args] = t.calls();
    assert.equal(args[args.indexOf('--frame-start') + 1], '15');
    assert.equal(args[args.indexOf('--frame-end') + 1], '15');
    assert.equal(args[args.indexOf('--codec') + 1], 'png');
  } finally {
    t.cleanup();
  }
});

test('renderer: renderStill validates its inputs and propagates CLI failures', async () => {
  const t = setup();
  try {
    await assert.rejects(
      renderStill({ composition: 'HelloWorld', output: join(t.dir, 'a.gif'), imageFormat: 'gif' }),
      /unsupported imageFormat/,
    );
    await assert.rejects(
      renderStill({ composition: 'HelloWorld', output: join(t.dir, 'a.png'), frame: -1 }),
      /non-negative integer/,
    );
    process.env.FAKE_MODE = 'fail';
    await assert.rejects(
      renderStill({ composition: 'Nope', output: join(t.dir, 'a.png') }),
      /exited with code 1/,
    );
  } finally {
    t.cleanup();
  }
});
