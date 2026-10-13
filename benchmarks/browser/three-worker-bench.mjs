//! Measure the persistent Chromium/Three.js worker without charging startup.
import {spawn} from 'node:child_process';
import {performance} from 'node:perf_hooks';
import {createInterface} from 'node:readline';
import {writeFileSync} from 'node:fs';

const root = new URL('../../', import.meta.url);
const worker = new URL('../../apps/studio-tauri/scripts/three-render-worker.mjs', import.meta.url);
const url = process.env.RUST_MOTION_BROWSER_URL ?? 'http://127.0.0.1:1421';
const frames = Number(process.env.FRAMES ?? 180);
const repeats = Number(process.env.REPEATS ?? 3);
const concurrency = Math.max(1, Number(process.env.WORKERS ?? 1));
const output = process.env.OUTPUT;
const node = process.env.RUST_MOTION_BROWSER_NODE ?? process.execPath;
const browser = process.env.CHROME_PATH;
const imageFormat = process.env.RUST_MOTION_BROWSER_IMAGE_FORMAT;
const jpegQuality = Number(process.env.RUST_MOTION_BROWSER_JPEG_QUALITY ?? 90);
const transparent = ['1', 'true', 'yes'].includes(
  (process.env.RUST_MOTION_BROWSER_TRANSPARENT ?? '').trim().toLowerCase(),
);
const transport = process.env.RUST_MOTION_BROWSER_TRANSPORT;

function spawnWorker() {
  return new Promise((resolve, reject) => {
    const args = [worker.pathname, `--url=${url}`];
    if (browser) args.push(`--browser=${browser}`);
    const child = spawn(node, args, {stdio: ['pipe', 'pipe', 'inherit']});
    const lines = createInterface({input: child.stdout});
    const pending = new Map();
    let ready = false;
    let failed;
    lines.on('line', (line) => {
      try {
        const message = JSON.parse(line);
        if (message.type === 'ready') ready = true;
        if (message.type === 'frame' || message.type === 'error') {
          const item = pending.get(message.frame);
          if (item) {
            pending.delete(message.frame);
            message.type === 'error' ? item.reject(new Error(message.message)) : item.resolve();
          }
        }
      } catch (error) { failed = error; }
    });
    child.once('error', reject);
    child.once('exit', (code) => { if (code !== 0 && !failed) failed = new Error(`worker exited with ${code}`); });
    const waitReady = () => ready ? Promise.resolve() : new Promise((r, j) => setTimeout(() => waitReady().then(r, j), 5));
    const request = (frame) => new Promise((resolveFrame, rejectFrame) => {
      pending.set(frame, {resolve: resolveFrame, reject: rejectFrame});
      child.stdin.write(JSON.stringify({
        type: 'render', composition: 'three_preview', frame, fps: 30,
        width: 1280, height: 720, props: {},
        ...(imageFormat === 'jpeg' ? {image_format: 'jpeg', jpeg_quality: jpegQuality} : {}),
        ...(transparent ? {transparent: true} : {}),
        ...(transport === 'file' || transport === 'rgba' || transport === 'rgba_file' ? {transport} : {}),
      }) + '\n');
    });
    (async () => {
      await waitReady();
      if (failed) throw failed;
      resolve({
        request,
        close: () => new Promise((resolveClose) => {
          if (child.exitCode !== null) return resolveClose();
          const timer = setTimeout(() => {
            child.kill('SIGTERM');
            resolveClose();
          }, 2000);
          child.once('exit', () => {
            clearTimeout(timer);
            resolveClose();
          });
          child.stdin.end(JSON.stringify({type: 'shutdown'}) + '\n');
        }),
      });
    })().catch((error) => { child.kill(); reject(error); });
  });
}

const workers = await Promise.all(Array.from({length: concurrency}, () => spawnWorker()));
const measure = async () => {
  const start = performance.now();
  for (let first = 0; first < frames; first += workers.length) {
    await Promise.all(workers.map((worker, index) => {
      const frame = first + index;
      return frame < frames ? worker.request(frame) : Promise.resolve();
    }));
  }
  return performance.now() - start;
};
await measure();
const samples = [];
try {
  for (let i = 0; i < repeats; i++) samples.push(await measure());
} finally {
  await Promise.all(workers.map((worker) => worker.close()));
}
const sorted = [...samples].sort((a, b) => a - b);
const report = {backend: 'chromium-three-worker', url, frames, width: 1280, height: 720, repeats, workers: concurrency, image_format: imageFormat ?? 'png', jpeg_quality: imageFormat === 'jpeg' ? jpegQuality : null, transport: transport ?? 'encoded', transparent, samples_ms: samples, median_ms: sorted[Math.floor(sorted.length / 2)], fps_equivalent: frames / (sorted[Math.floor(sorted.length / 2)] / 1000), node: process.version, platform: process.platform, arch: process.arch};
if (output) writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify(report, null, 2));
