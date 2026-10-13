import { createInterface } from 'node:readline';
import { Buffer } from 'node:buffer';
import { existsSync } from 'node:fs';
import { writeFileSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { chromium } from 'playwright-core';
import { PNG } from 'pngjs';

const args = new Map(process.argv.slice(2).flatMap((arg) => {
  const [key, value] = arg.split('=', 2);
  return key.startsWith('--') ? [[key.slice(2), value ?? '']] : [];
}));
const url = args.get('url') ?? 'http://localhost:1420';
const compositionModule = args.get('composition-module') ?? process.env.RUST_MOTION_BROWSER_COMPOSITION_MODULE;
const executablePath = args.get('browser') ?? process.env.CHROME_PATH ?? findBrowserExecutable();
const configuredFrameTimeoutMs = Number(args.get('frame-timeout-ms') ?? process.env.RUST_MOTION_BROWSER_FRAME_TIMEOUT_MS ?? 30000);
const frameTimeoutMs = Number.isFinite(configuredFrameTimeoutMs) && configuredFrameTimeoutMs > 0
  ? configuredFrameTimeoutMs
  : 30000;
const configuredFrameRetries = Number(
  args.get('frame-retries')
    ?? process.env.RUST_MOTION_BROWSER_TRANSPORT_RETRIES
    ?? process.env.RUST_MOTION_BROWSER_FRAME_RETRIES
    ?? 1,
);
const frameRetries = Number.isInteger(configuredFrameRetries) && configuredFrameRetries >= 0
  ? configuredFrameRetries
  : 1;
const write = (message) => process.stdout.write(`${JSON.stringify(message)}\n`);
const pendingFrameFiles = new Set();
const cleanupFrameFiles = () => {
  for (const path of pendingFrameFiles) {
    try { unlinkSync(path); } catch {}
  }
  pendingFrameFiles.clear();
};
const renderFrame = (request) => new Promise((resolve, reject) => {
  const timer = setTimeout(
    () => reject(new Error(`renderFrame timed out after ${frameTimeoutMs}ms`)),
    frameTimeoutMs,
  );
  page.evaluate((value) => window.rust_motion.renderFrame(value), request)
    .then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
});

const browser = await chromium.launch({
  ...(executablePath ? { executablePath } : {}),
  headless: true,
  args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage'],
});
const page = await browser.newPage({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1 });
let viewportWidth = 1280;
let viewportHeight = 720;
await page.addInitScript(() => { window.__RUST_MOTION_HEADLESS_RENDER__ = true; });
// Dev servers keep HMR/websocket connections open, so networkidle can never
// settle. The explicit renderFrame readiness check below is the real gate.
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: frameTimeoutMs });
if (compositionModule) {
  await page.evaluate(async (moduleUrl) => {
    const loadedModule = await eval('import(moduleUrl)');
    const api = window.rust-motion;
    if (typeof loadedModule.register === 'function') await loadedModule.register(api);
    const entries = loadedModule.compositions ?? loadedModule.default;
    if (entries && typeof entries === 'object' && !Array.isArray(entries)) {
      for (const [id, render] of Object.entries(entries)) {
        if (typeof render === 'function') api.registerComposition(id, render);
      }
    }
  }, compositionModule);
}
await page.waitForFunction(() => typeof window.rust-motion?.renderFrame === 'function', {
  timeout: frameTimeoutMs,
});
const compositions = await page.evaluate(() => window.rust_motion.listCompositions?.() ?? []);
write({ type: 'ready', protocol: 1, compositions });

const rl = createInterface({ input: process.stdin, terminal: false });
let queue = Promise.resolve();
rl.on('line', (line) => { queue = queue.then(async () => {
  let message;
  try { message = JSON.parse(line); } catch { return write({ type: 'error', frame: null, message: 'invalid JSON' }); }
  if (message.type === 'shutdown') { cleanupFrameFiles(); await browser.close(); process.exit(0); }
  if (message.type !== 'render') return;
  try {
    const request = message;
    let pageError;
    const onPageError = (error) => { pageError = error; };
    page.on('pageerror', onPageError);
    let lastError;
    try {
      for (let attempt = 0; attempt <= frameRetries; attempt += 1) {
        try {
          if (request.width !== viewportWidth || request.height !== viewportHeight) {
            await page.setViewportSize({ width: request.width, height: request.height });
            viewportWidth = request.width;
            viewportHeight = request.height;
          }
          await renderFrame(request);
          if (pageError) throw pageError;
          lastError = undefined;
          break;
        } catch (error) {
          lastError = error;
        }
      }
    } finally {
      page.off('pageerror', onPageError);
    }
    if (lastError) throw lastError;
    const imageType = request.image_format === 'jpeg' ? 'jpeg' : 'png';
    const fileTransport = request.transport === 'file';
    const rgbaTransport = request.transport === 'rgba';
    const rgbaFileTransport = request.transport === 'rgba_file';
    if (rgbaTransport || rgbaFileTransport) {
      const directRgba = await page.evaluate(async () => {
        const canvas = [...document.querySelectorAll('canvas')]
          .sort((a, b) => (b.width * b.height) - (a.width * a.height))[0];
        if (!canvas || canvas.width <= 0 || canvas.height <= 0) return null;
        const width = canvas.width;
        const height = canvas.height;
        if (typeof VideoFrame === 'function') {
          const frame = new VideoFrame(canvas, { timestamp: 0 });
          try {
            const displayWidth = frame.displayWidth;
            const displayHeight = frame.displayHeight;
            const pixels = new Uint8Array(displayWidth * displayHeight * 4);
            await frame.copyTo(pixels, {
              format: 'RGBA',
              layout: [{ offset: 0, stride: displayWidth * 4 }],
            });
            let binary = '';
            const chunkSize = 0x8000;
            for (let offset = 0; offset < pixels.length; offset += chunkSize) {
              binary += String.fromCharCode(...pixels.subarray(offset, offset + chunkSize));
            }
            return {
              width: displayWidth,
              height: displayHeight,
              rgba_base64: btoa(binary),
              transport: 'webcodecs',
            };
          } finally {
            frame.close();
          }
        }
        let pixels;
        const gl = canvas.getContext('webgl2') || canvas.getContext('webgl');
        if (gl) {
          pixels = new Uint8Array(width * height * 4);
          gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
          const row = new Uint8Array(width * 4);
          for (let y = 0; y < Math.floor(height / 2); y += 1) {
            const top = y * row.length;
            const bottom = (height - 1 - y) * row.length;
            row.set(pixels.subarray(top, top + row.length));
            pixels.copyWithin(top, bottom, bottom + row.length);
            pixels.set(row, bottom);
          }
        } else {
          const context = canvas.getContext('2d', { willReadFrequently: true });
          if (!context) return null;
          pixels = context.getImageData(0, 0, width, height).data;
        }
        let binary = '';
        const chunkSize = 0x8000;
        for (let offset = 0; offset < pixels.length; offset += chunkSize) {
          binary += String.fromCharCode(...pixels.subarray(offset, offset + chunkSize));
        }
        return { width, height, rgba_base64: btoa(binary) };
      });
      if (directRgba) {
        let videoFrame = directRgba;
        if (rgbaFileTransport) {
          const path = `${tmpdir()}/rust-motion-rgba-${process.pid}-${request.frame}-${Date.now()}.bin`;
          writeFileSync(path, Buffer.from(directRgba.rgba_base64, 'base64'));
          pendingFrameFiles.add(path);
          videoFrame = {width: directRgba.width, height: directRgba.height, file_path: path};
        }
        write({ type: 'frame', frame: request.frame, width: directRgba.width, height: directRgba.height,
          transport: directRgba.transport ?? 'canvas-readback',
          video_frame: {
            ...videoFrame,
            timestamp_us: Math.round((request.frame / Math.max(request.fps ?? 30, 1)) * 1_000_000),
          } });
        return;
      }
    }
    const screenshot = await page.screenshot({ type: imageType, ...(fileTransport || rgbaTransport ? {} : { encoding: 'base64' }),
      omitBackground: imageType === 'png' && request.transparent === true,
      quality: imageType === 'jpeg' ? (request.jpeg_quality ?? 90) : undefined });
    if (fileTransport) {
      const path = `${tmpdir()}/rust-motion-frame-${process.pid}-${request.frame}-${Date.now()}.${imageType}`;
      writeFileSync(path, screenshot);
      pendingFrameFiles.add(path);
      write({ type: 'frame', frame: request.frame, width: request.width, height: request.height, file_path: path });
    } else if (rgbaTransport) {
      if (imageType !== 'png') throw new Error('rgba transport requires PNG screenshot encoding');
      const decoded = PNG.sync.read(screenshot);
      write({ type: 'frame', frame: request.frame, width: decoded.width, height: decoded.height,
        video_frame: {
          width: decoded.width,
          height: decoded.height,
          timestamp_us: Math.round((request.frame / Math.max(request.fps ?? 30, 1)) * 1_000_000),
          rgba_base64: Buffer.from(decoded.data).toString('base64'),
        } });
    } else {
    const encodedScreenshot = Buffer.isBuffer(screenshot)
      ? screenshot.toString('base64')
      : screenshot;
    write({ type: 'frame', frame: request.frame, width: request.width, height: request.height,
      ...(imageType === 'png'
        ? { png_base64: encodedScreenshot }
        : { jpeg_base64: encodedScreenshot }) });
    }
  } catch (error) {
    write({ type: 'error', frame: message.frame ?? null, message: String(error) });
  }
}); });

process.once('SIGTERM', async () => { cleanupFrameFiles(); await browser.close(); process.exit(0); });

function findBrowserExecutable() {
  const candidates = process.platform === 'darwin'
    ? ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
      '/Applications/Chromium.app/Contents/MacOS/Chromium', 'google-chrome', 'chromium']
    : process.platform === 'win32'
      ? [
        `${process.env.PROGRAMFILES ?? 'C:\\Program Files'}\\Google\\Chrome\\Application\\chrome.exe`,
        `${process.env['PROGRAMFILES(X86)'] ?? 'C:\\Program Files (x86)'}\\Google\\Chrome\\Application\\chrome.exe`,
        `${process.env.LOCALAPPDATA ?? ''}\\Google\\Chrome\\Application\\chrome.exe`,
        'chrome.exe',
      ]
      : ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser'];
  const pathEntries = (process.env.PATH ?? '').split(process.platform === 'win32' ? ';' : ':');
  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
    if (!candidate.includes('/') && !candidate.includes('\\')) {
      const onPath = pathEntries.map((entry) => `${entry}/${candidate}`).find(existsSync);
      if (onPath) return onPath;
    }
  }
  return undefined;
}
