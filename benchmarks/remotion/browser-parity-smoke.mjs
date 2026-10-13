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
const frames = [0, 1, 7, 15, 30, 59];
const output = path.join(root, 'target/browser-parity-smoke');
mkdirSync(output, {recursive: true});
const serveUrl = await bundle({
  entryPoint: path.join(root, 'benchmarks/remotion/browser-parity-scene.tsx'),
  outDir: path.join(output, 'bundle'),
});
const remotionBrowser = await openBrowser('chrome', {browserExecutable, logLevel: 'error'});
const remotionCommon = {serveUrl, puppeteerInstance: remotionBrowser, logLevel: 'error'};
const composition = await selectComposition({...remotionCommon, id: 'SpringRectsBrowserParity'});
const dioxusBrowser = await chromium.launch({executablePath: browserExecutable, headless: true});
const page = await dioxusBrowser.newPage({viewport: {width: 320, height: 180}, deviceScaleFactor: 1});
await page.addInitScript(() => { window.__RUST_MOTION_HEADLESS_RENDER__ = true; });
await page.goto(url, {waitUntil: 'domcontentloaded'});
await page.waitForFunction(() => typeof (window.rust_motion ?? window['rust-motion'])?.renderFrame === 'function');

try {
  const comparisons = [];
  let dioxusTotalMs = 0;
  let remotionTotalMs = 0;
  for (const frame of frames) {
    const dioxusPath = path.join(output, `rust-motion-${frame}.png`);
    const remotionPath = path.join(output, `remotion-${frame}.png`);
    const dioxusStart = performance.now();
    await page.evaluate((nextFrame) => window.rust_motion.renderFrame({
      composition: 'SpringRects', frame: nextFrame, fps: 30, width: 320, height: 180,
      durationInFrames: 60, props: {}, assets: [], timeline: [],
    }), frame);
    const dioxusPng = await page.evaluate(() => document.querySelector('#preview-canvas').toDataURL('image/png'));
    writeFileSync(dioxusPath, Buffer.from(dioxusPng.split(',')[1], 'base64'));
    const dioxusMs = performance.now() - dioxusStart;
    const remotionStart = performance.now();
    await renderStill({...remotionCommon, composition, frame, imageFormat: 'png', output: remotionPath});
    const remotionMs = performance.now() - remotionStart;
    dioxusTotalMs += dioxusMs;
    remotionTotalMs += remotionMs;
    const dioxusPixels = readFileSync(dioxusPath);
    const remotionPixels = readFileSync(remotionPath);
    const dioxusDecoded = PNG.sync.read(dioxusPixels);
    const remotionDecoded = PNG.sync.read(remotionPixels);
    const identical = dioxusDecoded.width === remotionDecoded.width
      && dioxusDecoded.height === remotionDecoded.height
      && dioxusDecoded.data.equals(remotionDecoded.data);
    comparisons.push({frame, identical, dioxusMs, remotionMs, width: dioxusDecoded.width,
      height: dioxusDecoded.height, dioxusBytes: dioxusDecoded.data.length,
      remotionBytes: remotionDecoded.data.length});
    assert.equal(identical, true, `PNG bytes differ at frame ${frame}`);
  }
  console.log(JSON.stringify({frames, comparisons, totals: {
    dioxusTotalMs, remotionTotalMs,
    dioxusMeanMs: dioxusTotalMs / frames.length,
    remotionMeanMs: remotionTotalMs / frames.length,
  }}, null, 2));
} finally {
  await dioxusBrowser.close();
  await remotionBrowser.close({silent: true});
}
