import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import path from 'node:path';
import {performance} from 'node:perf_hooks';
import {bundle} from '@remotion/bundler';
import {openBrowser, renderStill, selectComposition} from '@remotion/renderer';

const root = process.cwd();
const require = createRequire(new URL('../../apps/studio-tauri/package.json', import.meta.url));
const {chromium} = require('playwright-core');
const {PNG} = require('pngjs');
const browserExecutable = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const url = process.env.RUST_MOTION_BROWSER_URL ?? 'http://127.0.0.1:1420';
const frames = [0, 1, 15, 30, 59];
const output = path.join(root, 'target/browser-three-parity-smoke');
mkdirSync(output, {recursive: true});
const serveUrl = await bundle({entryPoint: path.join(root, 'benchmarks/remotion/three-parity-scene.tsx'), outDir: path.join(output, 'bundle')});
const dioxusBrowser = await chromium.launch({executablePath: browserExecutable, headless: true});
const page = await dioxusBrowser.newPage({viewport: {width: 640, height: 360}, deviceScaleFactor: 1});
await page.addInitScript(() => { window.__RUST_MOTION_HEADLESS_RENDER__ = true; });
await page.goto(url, {waitUntil: 'domcontentloaded'});
await page.waitForFunction(() => typeof window.rust-motion?.renderFrame === 'function');
const dioxusTimings = new Map();
try {
  for (const frame of frames) {
    const dioxusStart = performance.now();
    await page.evaluate((nextFrame) => window.rust_motion.renderFrame({composition: 'three_lifecycle_preview', frame: nextFrame, fps: 30, width: 640, height: 360, props: {}}), frame);
    const dioxusData = await page.evaluate(() => document.querySelector('#preview-canvas').toDataURL('image/png'));
    dioxusTimings.set(frame, performance.now() - dioxusStart);
    const dioxusPath = path.join(output, `dioxus-${frame}.png`);
    const dioxus = Buffer.from(dioxusData.split(',')[1], 'base64');
    writeFileSync(dioxusPath, dioxus);
  }
} finally {
  await dioxusBrowser.close();
}

const remotionBrowser = await openBrowser('chrome', {browserExecutable, logLevel: 'error'});
const common = {serveUrl, puppeteerInstance: remotionBrowser, logLevel: 'error'};
const composition = await selectComposition({...common, id: 'ThreeLifecycleParity'});
try {
  const comparisons = [];
  for (const frame of frames) {
    const remotionPath = path.join(output, `remotion-${frame}.png`);
    const remotionStart = performance.now();
    await renderStill({...common, composition, frame, imageFormat: 'png', output: remotionPath});
    const remotionMs = performance.now() - remotionStart;
    const dioxus = PNG.sync.read(readFileSync(path.join(output, `dioxus-${frame}.png`)));
    const remotion = PNG.sync.read(readFileSync(remotionPath));
    assert.equal(dioxus.width, remotion.width);
    assert.equal(dioxus.height, remotion.height);
    assert.deepEqual(dioxus.data, remotion.data, `Three.js pixels differ at frame ${frame}`);
    comparisons.push({frame, identical: true, dioxusMs: dioxusTimings.get(frame), remotionMs});
  }
  console.log(JSON.stringify({frames, comparisons, status: 'ok'}, null, 2));
} finally {
  await remotionBrowser.close({silent: true});
}
