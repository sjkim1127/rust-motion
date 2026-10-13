import { createRequire } from 'node:module';

const require = createRequire(new URL('../../apps/studio-tauri/package.json', import.meta.url));
const { chromium } = require('playwright-core');

const url = process.env.RUST_MOTION_BROWSER_URL ?? 'http://127.0.0.1:1420';
const executablePath = process.env.CHROME_PATH ??
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const jsonAsset = new URL('/package.json', url).toString();
const canvasAsset = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=';
const retryAssetPath = '/canvas-retry.png';

const browser = await chromium.launch({ executablePath, headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 320, height: 180 } });
  let retryRequests = 0;
  await page.route(`**${retryAssetPath}`, async (route) => {
    retryRequests += 1;
    if (retryRequests === 1) {
      await route.abort('failed');
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: 'image/png',
      body: Buffer.from(canvasAsset.split(',')[1], 'base64'),
    });
  });
  await page.addInitScript(() => { window.__RUST_MOTION_HEADLESS_RENDER__ = true; });
  await page.goto(url, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => typeof (window.rust_motion ?? window['rust-motion'])?.renderFrame === 'function');

  const result = await page.evaluate(async ({ jsonAsset, canvasAsset, retryAssetPath }) => {
    let compositionContext;
    let hookContext;
    window.rust_motion.registerComposition('media_sync_smoke', async (context) => {
      compositionContext = context;
      hookContext = {
        frame: window.rust_motion.useCurrentFrame(),
        config: window.rust_motion.useVideoConfig(),
        inputProps: window.rust_motion.getInputProps(),
        environment: window.rust_motion.useRemotionEnvironment(),
        staticFile: window.rust_motion.staticFile('assets/demo.png'),
        staticFiles: window.rust_motion.getStaticFiles(),
      };
      const { frame } = context;
      let media = document.querySelector('video[data-rust-motion-smoke]');
      if (!media) {
        media = document.createElement('video');
        media.dataset.rustMotionSmoke = '1';
        document.body.append(media);
      }

      let retryCanvas = document.querySelector('[data-rust-motion-canvas-image-retry-smoke]');
      if (!retryCanvas) {
        retryCanvas = document.createElement('canvas');
        retryCanvas.dataset.rustMotionCanvasImageRetrySmoke = '1';
        retryCanvas.dataset.rustMotionCanvasImage = 'true';
        retryCanvas.dataset.src = new URL(retryAssetPath, location.href).toString();
        retryCanvas.dataset.fit = 'fill';
        retryCanvas.dataset.maxRetries = '1';
        retryCanvas.dataset.pauseWhenLoading = 'true';
        retryCanvas.setAttribute('width', '1');
        retryCanvas.setAttribute('height', '1');
        document.body.append(retryCanvas);
      }
      media.dataset.time = String(frame / 10);
      media.dataset.timelineStart = '1';
      media.dataset.duration = '2';
      media.dataset.volume = '0.35';
      media.dataset.playbackRate = '1.5';
      media.setAttribute('loop', '');

      let lottie = document.querySelector('[data-rust-motion-lottie-smoke]');
      if (!lottie) {
        lottie = document.createElement('div');
        lottie.dataset.rustMotionLottieSmoke = '1';
        lottie.dataset.rustMotionLottie = `data:application/json,${encodeURIComponent(JSON.stringify({
          v: '5.5.7', fr: 30, ip: 0, op: 30, w: 16, h: 16, nm: 'smoke', ddd: 0,
          assets: [], layers: [],
        }))}`;
        lottie.dataset.playbackRate = '1';
        lottie.dataset.time = String(frame / 10);
        lottie.dataset.loop = 'Loop';
        document.body.append(lottie);
      }
      lottie.dataset.time = String(frame / 10);

      let canvasImage = document.querySelector('[data-rust-motion-canvas-image-smoke]');
      if (!canvasImage) {
        canvasImage = document.createElement('canvas');
        canvasImage.dataset.rustMotionCanvasImageSmoke = '1';
        canvasImage.dataset.rustMotionCanvasImage = 'true';
        canvasImage.dataset.src = canvasAsset;
        canvasImage.dataset.fit = 'contain';
        canvasImage.dataset.maxRetries = '2';
        canvasImage.dataset.pauseWhenLoading = 'true';
        canvasImage.setAttribute('width', '1');
        canvasImage.setAttribute('height', '1');
        document.body.append(canvasImage);
      }
    });

    await window.rust_motion.renderFrame({
      composition: 'media_sync_smoke', frame: 15, fps: 10,
      props: { title: 'smoke', nested: { value: 1 } }, assets: [jsonAsset], timeline: [], width: 320, height: 180, durationInFrames: 60,
    });
    const media = document.querySelector('video[data-rust-motion-smoke]');
    const lottie = document.querySelector('[data-rust-motion-lottie-smoke]');
    const active = {
      time: media.currentTime,
      visibility: media.style.visibility,
      volume: media.volume,
      rate: media.playbackRate,
      loop: media.loop,
    };

    await window.rust_motion.renderFrame({
      composition: 'media_sync_smoke', frame: 40, fps: 10,
      props: { title: 'smoke', nested: { value: 1 } }, assets: [jsonAsset], timeline: [], width: 320, height: 180, durationInFrames: 60,
    });
    const imageDimensions = await window.rust_motion.getImageDimensions(canvasAsset);
    const cachedImageDimensions = await window.rust_motion.getImageDimensions(canvasAsset);
    return {
      active,
      inactiveVisibility: media.style.visibility,
      lottieSvg: Boolean(lottie.querySelector('svg')),
      lottieFrame: lottie.dataset.frame,
      canvasPixel: [...document.querySelector('[data-rust-motion-canvas-image-smoke]').getContext('2d').getImageData(0, 0, 1, 1).data],
      retryCanvasPixel: [...document.querySelector('[data-rust-motion-canvas-image-retry-smoke]').getContext('2d').getImageData(0, 0, 1, 1).data],
      canvasRetries: document.querySelector('[data-rust-motion-canvas-image-smoke]').dataset.maxRetries,
      canvasPause: document.querySelector('[data-rust-motion-canvas-image-smoke]').dataset.pauseWhenLoading,
      compositionContext: {
        width: compositionContext.width,
        height: compositionContext.height,
        durationInFrames: compositionContext.durationInFrames,
      },
      hookContext,
      imageDimensions,
      cachedImageDimensions,
    };
  }, { jsonAsset, canvasAsset, retryAssetPath });

  const assert = (condition, message) => {
    if (!condition) throw new Error(message);
  };
  assert(Math.abs(result.active.time - 1.5) < 0.001, `unexpected currentTime: ${result.active.time}`);
  assert(result.active.visibility === '', `active media is hidden: ${result.active.visibility}`);
  assert(Math.abs(result.active.volume - 0.35) < 0.001, `unexpected volume: ${result.active.volume}`);
  assert(Math.abs(result.active.rate - 1.5) < 0.001, `unexpected playbackRate: ${result.active.rate}`);
  assert(result.active.loop, 'loop was not enabled');
  assert(result.inactiveVisibility === 'hidden', 'inactive media was not hidden');
  assert(result.lottieSvg, 'Lottie adapter did not create an SVG');
  assert(result.lottieFrame === '40', `unexpected Lottie frame: ${result.lottieFrame}`);
  assert(result.canvasPixel[3] > 0, `CanvasImage was not rasterized: ${result.canvasPixel}`);
  assert(result.canvasRetries === '2', `unexpected CanvasImage retries: ${result.canvasRetries}`);
  assert(result.canvasPause === 'true', 'CanvasImage loading policy was not preserved');
  assert(retryRequests === 2, `CanvasImage did not retry after failure: ${retryRequests}`);
  assert(result.retryCanvasPixel[3] > 0, `CanvasImage retry did not rasterize: ${result.retryCanvasPixel}`);
  assert(result.compositionContext.width === 320, `unexpected composition width: ${result.compositionContext.width}`);
  assert(result.compositionContext.height === 180, `unexpected composition height: ${result.compositionContext.height}`);
  assert(result.compositionContext.durationInFrames === 60,
    `unexpected composition duration: ${result.compositionContext.durationInFrames}`);
  assert(result.hookContext.frame === 40, `unexpected hook frame: ${result.hookContext.frame}`);
  assert(result.hookContext.config.fps === 10 && result.hookContext.config.width === 320 &&
    result.hookContext.config.height === 180 && result.hookContext.config.durationInFrames === 60,
  `unexpected hook video config: ${JSON.stringify(result.hookContext.config)}`);
  assert(result.hookContext.staticFile.endsWith('/assets/demo.png') && result.hookContext.staticFile.startsWith('http'),
    `unexpected staticFile URL: ${result.hookContext.staticFile}`);
  assert(result.hookContext.staticFiles.length === 1 && result.hookContext.staticFiles[0].endsWith('/package.json'),
    'static files did not expose the active request assets');
  assert(result.hookContext.inputProps.title === 'smoke' && result.hookContext.inputProps.nested.value === 1,
    `input props were not synchronized: ${JSON.stringify(result.hookContext.inputProps)}`);
  assert(result.hookContext.environment.isRendering && !result.hookContext.environment.isPlayer,
    `unexpected render environment: ${JSON.stringify(result.hookContext.environment)}`);
  assert(result.imageDimensions.width === 1 && result.imageDimensions.height === 1,
    `unexpected image dimensions: ${JSON.stringify(result.imageDimensions)}`);
  assert(result.cachedImageDimensions.width === 1 && result.cachedImageDimensions.height === 1,
    `cached image dimensions failed: ${JSON.stringify(result.cachedImageDimensions)}`);
  console.log('browser media sync smoke: passed');
} finally {
  await browser.close();
}
