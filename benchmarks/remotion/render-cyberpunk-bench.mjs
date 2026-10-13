// Run from the repository root.
// Compares Remotion vs Rust Motion on a realistic 1080p Cyberpunk motion graphics scene
// exercising text auto-fitting, drop-shadow glow, offscreen layers, chromatic aberration, and vignette.
import {bundle} from '@remotion/bundler';
import {openBrowser, renderMedia, selectComposition} from '@remotion/renderer';
import {execFileSync} from 'node:child_process';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {performance} from 'node:perf_hooks';

const root = process.cwd();
const outputDir = path.join(root, 'target/remotion-benchmark-cyberpunk');
mkdirSync(outputDir, {recursive: true});

const nativeBinary = path.join(root, 'target/release/examples/cyberpunk_bench');
const browserExecutable = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

console.log('[*] Bundling Remotion Cyberpunk scene...');
const setupStart = performance.now();
const serveUrl = await bundle({
  entryPoint: path.join(root, 'benchmarks/remotion/cyberpunk_scene.tsx'),
  outDir: path.join(outputDir, 'bundle'),
});
const bundleMs = performance.now() - setupStart;

console.log('[*] Launching Chromium browser...');
const browserStart = performance.now();
const browser = await openBrowser('chrome', {browserExecutable, logLevel: 'error'});
const browserMs = performance.now() - browserStart;

const common = {serveUrl, puppeteerInstance: browser, logLevel: 'error'};

try {
  const composition = await selectComposition({...common, id: 'CyberpunkScene'});

  console.log('[*] Rendering Remotion Cyberpunk scene (1920x1080, 180 frames, concurrency: 4)...');
  const remotionStart = performance.now();
  await renderMedia({
    ...common,
    composition,
    outputLocation: path.join(outputDir, 'remotion-cyberpunk.mp4'),
    codec: 'h264',
    crf: 18,
    x264Preset: 'fast',
    pixelFormat: 'yuv420p',
    concurrency: 4,
    imageFormat: 'png',
    muted: true,
  });
  const remotionMs = performance.now() - remotionStart;
  console.log(`[+] Remotion render completed in ${(remotionMs / 1000).toFixed(2)}s`);

  console.log('[*] Rendering Rust Motion Cyberpunk scene (1920x1080, 180 frames, concurrency: 4)...');
  const rustMotionStart = performance.now();
  const rawOutput = execFileSync(nativeBinary, [path.join(outputDir, 'rust-motion-cyberpunk.mp4'), '4'], {
    encoding: 'utf8',
  });
  const rustMotionProcessMs = performance.now() - rustMotionStart;
  const nativeStats = JSON.parse(rawOutput.trim());
  console.log(`[+] Rust Motion render completed in ${(rustMotionProcessMs / 1000).toFixed(2)}s (internal render: ${(nativeStats.render_ms / 1000).toFixed(2)}s)`);

  const nativeOutput = path.join(outputDir, 'rust-motion-cyberpunk.mp4');
  const remotionOutput = path.join(outputDir, 'remotion-cyberpunk.mp4');
  const probe = (file) => JSON.parse(execFileSync('ffprobe', [
    '-v', 'error', '-count_frames', '-select_streams', 'v:0',
    '-show_entries', 'stream=width,height,nb_read_frames,r_frame_rate,codec_name,pix_fmt',
    '-of', 'json', file,
  ], {encoding: 'utf8'})).streams[0];
  const probes = {native: probe(nativeOutput), remotion: probe(remotionOutput)};
  for (const [name, metadata] of Object.entries(probes)) {
    if (metadata.width !== 1920 || metadata.height !== 1080
      || metadata.nb_read_frames !== '180' || metadata.r_frame_rate !== '30/1'
      || metadata.codec_name !== 'h264' || !['yuv420p', 'yuvj420p'].includes(metadata.pix_fmt)) {
      throw new Error(`Unexpected ${name} output metadata: ${JSON.stringify(metadata)}`);
    }
  }
  const ssimPath = path.join(outputDir, 'cyberpunk-ssim.log');
  execFileSync('ffmpeg', [
    '-v', 'error', '-i', nativeOutput, '-i', remotionOutput,
    '-lavfi', `[0:v]scale=in_range=auto:out_range=tv,format=yuv420p[a];[1:v]scale=in_range=auto:out_range=tv,format=yuv420p[b];[a][b]ssim=stats_file=${ssimPath}`,
    '-f', 'null', '-',
  ]);
  const ssimScores = [...readFileSync(ssimPath, 'utf8').matchAll(/All:([0-9.]+)/g)]
    .map((match) => Number(match[1]));
  const decodedSsim = {
    frames: ssimScores.length,
    min: Math.min(...ssimScores),
    mean: ssimScores.reduce((sum, score) => sum + score, 0) / ssimScores.length,
  };
  const parityGatePassed = decodedSsim.frames === 180 && decodedSsim.min >= 0.995;
  const cropSsim = (name, x, y, width, height) => {
    const cropPath = path.join(outputDir, `cyberpunk-ssim-${name}.log`);
    execFileSync('ffmpeg', [
      '-v', 'error', '-i', nativeOutput, '-i', remotionOutput,
      '-lavfi', `[0:v]crop=${width}:${height}:${x}:${y},format=yuv420p[a];[1:v]crop=${width}:${height}:${x}:${y},format=yuv420p[b];[a][b]ssim=stats_file=${cropPath}`,
      '-f', 'null', '-',
    ]);
    const scores = [...readFileSync(cropPath, 'utf8').matchAll(/All:([0-9.]+)/g)]
      .map((match) => Number(match[1]));
    return {frames: scores.length, min: Math.min(...scores), mean: scores.reduce((sum, score) => sum + score, 0) / scores.length};
  };
  const cropValidation = {
    title: cropSsim('title', 0, 320, 1100, 330),
    badge: cropSsim('badge', 160, 700, 450, 130),
    background: cropSsim('background', 0, 0, 1920, 300),
  };

  const speedup = remotionMs / rustMotionProcessMs;
  console.log(`\n======================================================`);
  console.log(`🔥 1080p VFX Speedup: Rust Motion is ${speedup.toFixed(2)}x faster!`);
  console.log(`======================================================\n`);

  const report = {
    workload: '1080p Cyberpunk Title Reel (1920x1080, 30fps, 180 frames)',
    features: ['Text Auto-Fitting', 'Drop-Shadow Glow', 'Chromatic Aberration', 'Vignette', 'Layer Composition'],
    concurrency: 4,
    platform: `${os.platform()} (${os.arch()})`,
    cpu: os.cpus()[0].model,
    bundle_ms: bundleMs,
    browser_startup_ms: browserMs,
    remotion: {
      render_ms: remotionMs,
      fps: 180 / (remotionMs / 1000),
    },
    'rust-motion': {
      process_ms: rustMotionProcessMs,
      render_ms: nativeStats.render_ms,
      fps: 180 / (rustMotionProcessMs / 1000),
    },
    speedup,
    validation: {probes, decoded_ssim: decodedSsim, parity_gate_passed: parityGatePassed, crop_ssim: cropValidation, ffmpeg: execFileSync('ffmpeg', ['-version'], {encoding: 'utf8'}).split('\n')[0]},
  };

  const reportPath = path.join(root, 'benchmarks/cyberpunk-bench-result.json');
  writeFileSync(reportPath, JSON.stringify(report, null, 2) + '\n');
  console.log(`[+] Report saved to ${reportPath}`);
  if (!parityGatePassed) {
    throw new Error(`Cyberpunk decoded SSIM gate failed: ${JSON.stringify(decodedSsim)}`);
  }
} finally {
  await browser.close({silent: true});
}
