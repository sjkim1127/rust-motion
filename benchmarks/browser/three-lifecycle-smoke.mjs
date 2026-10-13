import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFileSync, unlinkSync } from 'node:fs';
import { createInterface } from 'node:readline';

const url = process.env.RUST_MOTION_BROWSER_URL ?? 'http://127.0.0.1:1421';
const worker = new URL('../../apps/studio-tauri/scripts/three-render-worker.mjs', import.meta.url);
const moduleUrl = `${url}/src/three-lifecycle-composition.js`;
const child = spawn(process.execPath, [worker.pathname, `--url=${url}`, `--composition-module=${moduleUrl}`], {
  stdio: ['pipe', 'pipe', 'inherit'],
});
let childExited = false;
const childExit = new Promise((resolve) => {
  child.once('exit', (code, signal) => {
    childExited = true;
    resolve({ code, signal });
  });
  child.once('error', (error) => {
    childExited = true;
    resolve({ error });
  });
});
const lines = createInterface({ input: child.stdout });
const messages = [];
lines.on('line', (line) => {
  try {
    messages.push(JSON.parse(line));
  } catch (err) {
    console.error('NON_JSON LINE:', line);
  }
});
const waitFor = async (predicate, timeoutMs = 15000) => {
  const started = Date.now();
  while (true) {
    const index = messages.findIndex(predicate);
    if (index >= 0) return messages.splice(index, 1)[0];
    if (childExited) {
      const exit = await childExit;
      const reason = exit.error?.message ?? `code ${exit.code}, signal ${exit.signal}`;
      throw new Error(`Browser worker exited before sending the expected message (${reason})`);
    }
    if (Date.now() - started > timeoutMs) {
      throw new Error(`Timed out waiting for message after ${timeoutMs}ms. Received messages: ${JSON.stringify(messages)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
};

try {
  const ready = await waitFor((message) => message.type === 'ready', 60_000);
  assert.ok(ready.compositions.includes('three_lifecycle_preview'));
  assert.ok(ready.compositions.includes('three_audio_reactive_preview'));
  assert.ok(ready.compositions.includes('timeline_layer_base'));
  assert.ok(ready.compositions.includes('timeline_layer_overlay'));
  const parityRequest = { type: 'render', composition: 'three_lifecycle_preview', frame: 1, fps: 30, width: 640, height: 360, props: { color: '#ff8844' } };
  child.stdin.write(`${JSON.stringify({ ...parityRequest, transport: 'rgba' })}\n`);
  const rgbaParity = await waitFor((message) => message.type === 'frame' && message.frame === 1);
  const rgbaBytes = Buffer.from(rgbaParity.video_frame.rgba_base64, 'base64');
  assert.equal(rgbaBytes.length, 640 * 360 * 4);
  child.stdin.write(`${JSON.stringify({ ...parityRequest, transport: 'rgba_file' })}\n`);
  const fileParity = await waitFor((message) => message.type === 'frame' && message.frame === 1);
  const fileBytes = readFileSync(fileParity.video_frame.file_path);
  assert.deepEqual(fileBytes, rgbaBytes);
  unlinkSync(fileParity.video_frame.file_path);
  for (const frame of [0, 15, 30]) {
    child.stdin.write(`${JSON.stringify({ type: 'render', composition: 'three_lifecycle_preview', frame, fps: 30, width: 640, height: 360, props: { color: '#ff8844' }, ...(frame === 0 ? { transport: 'rgba' } : frame === 15 ? { transport: 'rgba_file' } : {}) })}\n`);
    const response = await waitFor((message) => message.type === 'frame' && message.frame === frame);
    assert.equal(response.width, 640);
    assert.equal(response.height, 360);
    if (frame === 0) {
      assert.equal(response.video_frame.width, 640);
      assert.equal(response.video_frame.height, 360);
      assert.equal(Buffer.from(response.video_frame.rgba_base64, 'base64').length, 640 * 360 * 4);
    }
    if (frame === 15) {
      assert.equal(response.video_frame.width, 640);
      assert.equal(response.video_frame.height, 360);
      assert.match(response.video_frame.file_path, /rust-motion-rgba-/);
      assert.equal(readFileSync(response.video_frame.file_path).length, 640 * 360 * 4);
      unlinkSync(response.video_frame.file_path);
    }
  }
  for (const frame of [0, 15, 30]) {
    child.stdin.write(`${JSON.stringify({ type: 'render', composition: 'three_audio_reactive_preview', frame, fps: 30, width: 640, height: 360 })}\n`);
    const response = await waitFor((message) => message.type === 'frame' && message.frame === frame);
    assert.equal(response.width, 640);
    assert.equal(response.height, 360);
  }

  child.stdin.write(`${JSON.stringify({
    type: 'render', composition: 'timeline_hook_probe', frame: 55, fps: 10,
    width: 64, height: 64, durationInFrames: 100, props: { projectOnly: true },
    timeline: [{
      composition: 'timeline_hook_probe', start: 50, duration: 20,
      props: { clipOnly: 'visible' },
    }],
  })}\n`);
  const hookContext = await waitFor((message) =>
    (message.type === 'frame' || message.type === 'error') && message.frame === 55);
  assert.equal(hookContext.type, 'frame', `timeline hook context failed: ${hookContext.message ?? 'no frame response'}`);

  child.stdin.write(`${JSON.stringify({
    type: 'render', composition: 'three_preview', frame: 1, fps: 30,
    width: 640, height: 360, durationInFrames: 10, props: {}, transport: 'rgba',
    timeline: [
      { composition: 'timeline_layer_base', start: 0, duration: 10, props: {} },
      { composition: 'timeline_layer_overlay', start: 0, duration: 10, props: {} },
    ],
  })}\n`);
  const layeredFrame = await waitFor((message) =>
    (message.type === 'frame' || message.type === 'error') && message.frame === 1);
  assert.equal(layeredFrame.type, 'frame', `timeline layer render failed: ${layeredFrame.message ?? 'no frame response'}`);
  assert.equal(layeredFrame.video_frame.width, 640,
    `timeline frame dimensions: ${layeredFrame.video_frame.width}x${layeredFrame.video_frame.height}`);
  assert.equal(layeredFrame.video_frame.height, 360,
    `timeline frame dimensions: ${layeredFrame.video_frame.width}x${layeredFrame.video_frame.height}`);
  const layeredPixels = Buffer.from(layeredFrame.video_frame.rgba_base64, 'base64');
  assert.equal(layeredPixels.length, 640 * 360 * 4, `timeline RGBA byte length: ${layeredPixels.length}`);
  const centerPixel = 4 * (Math.floor(180) * 640 + Math.floor(320));
  assert.ok(layeredPixels[centerPixel] > 80, `base layer was cleared: ${[...layeredPixels.subarray(centerPixel, centerPixel + 4)]}`);
  assert.ok(layeredPixels[centerPixel + 2] > 80, `overlay layer was not composited: ${[...layeredPixels.subarray(centerPixel, centerPixel + 4)]}`);
  assert.ok(layeredPixels[centerPixel + 1] < 20, `unexpected green channel: ${[...layeredPixels.subarray(centerPixel, centerPixel + 4)]}`);

  console.log(JSON.stringify({ compositions: 5, frames: 10, raw_rgba_parity: true, timeline_hooks: true, overlapping_layers: true, status: 'ok' }));
} finally {
  if (!childExited) {
    try { child.stdin.write('{"type":"shutdown"}\n'); } catch {}
    let exited = await Promise.race([
      childExit.then(() => true),
      new Promise((resolve) => setTimeout(() => resolve(false), 5000)),
    ]);
    if (!exited) {
      child.kill('SIGTERM');
      exited = await Promise.race([
        childExit.then(() => true),
        new Promise((resolve) => setTimeout(() => resolve(false), 2000)),
      ]);
    }
    if (!exited) {
      child.kill('SIGKILL');
      await Promise.race([
        childExit,
        new Promise((resolve) => setTimeout(resolve, 2000)),
      ]);
    }
  }
}
