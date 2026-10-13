// Run from the repository root. Build the Rust example before measuring.
import {bundle} from '@remotion/bundler';
import {openBrowser, renderMedia, renderStill, selectComposition} from '@remotion/renderer';
import {execFileSync} from 'node:child_process';
import {mkdirSync, readFileSync, readdirSync, existsSync, symlinkSync, unlinkSync, writeFileSync} from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {performance} from 'node:perf_hooks';
const root = process.cwd();
const imageFormat = process.env.IMAGE_FORMAT ?? 'png';
if (!['png','jpeg'].includes(imageFormat)) throw new Error('IMAGE_FORMAT must be png or jpeg');
const output = path.join(root, `target/remotion-benchmark-${imageFormat}`);
mkdirSync(output, {recursive:true});
const native = path.join(root, 'target/release/examples/render_bench');
const suffix = process.platform === 'linux' ? '-gnu' : process.platform === 'win32' ? '-msvc' : '';
const {dir:binaryDir} = await import(`@remotion/compositor-${process.platform}-${process.arch}${suffix}`);
// Remotion's bundled FFmpeg omits the rawvideo demuxer used by native export.
// Use system FFmpeg for both, retaining Remotion's own compositor and libraries.
const sharedBinaryDir = path.join(output, 'binaries');
mkdirSync(sharedBinaryDir, {recursive:true});
const ffmpeg = execFileSync('which', ['ffmpeg'], {encoding:'utf8'}).trim();
const ffprobe = execFileSync('which', ['ffprobe'], {encoding:'utf8'}).trim();
for (const name of readdirSync(binaryDir)) {
  const target = name === 'ffmpeg' ? ffmpeg : name === 'ffprobe' ? ffprobe : path.join(binaryDir, name);
  const link = path.join(sharedBinaryDir, name);
  if (existsSync(link)) unlinkSync(link);
  symlinkSync(target, link);
}
const nativeEnv = process.env;
const browserExecutable = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const setupStart = performance.now();
const serveUrl = await bundle({entryPoint:path.join(root,'benchmarks/remotion/scene.tsx'), outDir:path.join(output,'bundle')});
const bundleMs = performance.now() - setupStart;
const browserStart = performance.now();
const browser = await openBrowser('chrome', {browserExecutable, logLevel:'error'});
const browserMs = performance.now() - browserStart;
const common = {serveUrl, puppeteerInstance:browser, binariesDirectory:sharedBinaryDir, logLevel:'error'};
try {
  const composition = await selectComposition({...common, id:'SpringRects'});
  const remotion = async (name) => {
    const start = performance.now();
    await renderMedia({...common, composition, outputLocation:path.join(output,`${name}.mp4`), codec:'h264', crf:18, x264Preset:'fast', pixelFormat:'yuv420p', concurrency:4, imageFormat, muted:true});
    return performance.now() - start;
  };
  const rust = (name) => {
    const start = performance.now();
    const result = JSON.parse(execFileSync(native, [path.join(output,`${name}.mp4`)], {encoding:'utf8', env:nativeEnv}));
    return {process_ms:performance.now() - start, ...result};
  };
  // Exclude one warmup render for both. Chromium is reused for every Remotion render.
  const warmup = {rust-motion:rust('native-warmup'), remotion_ms:await remotion('remotion-warmup')};
  const samples = {rust-motion:[], remotion:[]};
  const repeats = Number(process.env.REPEATS ?? 3);
  const hostLoad = [];
  const nativeRenderMs = [];
  for (let i=0; i<repeats; i++) {
    for (const engine of (i % 2 === 0 ? ['rust-motion','remotion'] : ['remotion','rust-motion'])) {
      hostLoad.push({engine, run:i, load:os.loadavg()});
      let ms;
      if (engine === 'rust-motion') {
        const result = rust(`native-${i}`);
        nativeRenderMs.push(result.render_ms);
        ms = result.process_ms;
      } else {
        ms = await remotion(`remotion-${i}`);
      }
      samples[engine].push(ms);
      console.log(`${engine} run ${i+1}: ${ms.toFixed(1)} ms`);
    }
  }
  for (const frame of [0, 1, 7, 15, 30, 59, 60, 179]) {
    execFileSync(native, ['--still', String(frame), path.join(output,`native-${frame}.png`)], {env:nativeEnv});
    await renderStill({...common, composition, frame, imageFormat:'png', output:path.join(output,`remotion-${frame}.png`)});
  }
  const decode = file => execFileSync('ffmpeg', ['-v','error','-i',file,'-f','rawvideo','-pix_fmt','rgba','pipe:1'], {maxBuffer:8*1024*1024});
  const pixelChecks = [];
  for (const frame of [0, 1, 7, 15, 30, 59, 60, 179]) {
    const a = decode(path.join(output,`native-${frame}.png`));
    const b = decode(path.join(output,`remotion-${frame}.png`));
    if (!a.equals(b)) throw new Error(`PNG pixels differ at frame ${frame}`);
    pixelChecks.push({frame, identical:true, rgba_bytes:a.length});
  }
  const probes = {};
  for (const engine of ['native','remotion']) {
    probes[engine] = JSON.parse(execFileSync('ffprobe', ['-v','error','-count_frames','-select_streams','v:0','-show_entries','stream=width,height,nb_read_frames,r_frame_rate,codec_name,pix_fmt,color_range,color_space','-of','json',path.join(output,`${engine}-0.mp4`)], {encoding:'utf8'})).streams[0];
    const p = probes[engine];
    if (p.width !== 1280 || p.height !== 720 || p.nb_read_frames !== '180' || p.r_frame_rate !== '30/1' || p.codec_name !== 'h264' || !['yuv420p','yuvj420p'].includes(p.pix_fmt)) throw new Error(`Unexpected ${engine} output metadata`);
  }
  const stats = path.join(output,'ssim.log');
  execFileSync('ffmpeg', ['-v','error','-i',path.join(output,'native-0.mp4'),'-i',path.join(output,'remotion-0.mp4'),'-lavfi',`[0:v]scale=in_range=auto:out_range=tv,format=yuv420p[a];[1:v]scale=in_range=auto:out_range=tv,format=yuv420p[b];[a][b]ssim=stats_file=${stats}`,'-f','null','-']);
  const scores = [...readFileSync(stats,'utf8').matchAll(/All:([0-9.]+)/g)].map(match => Number(match[1]));
  if (scores.length !== 180 || Math.min(...scores) < 0.995) throw new Error('Decoded MP4 similarity gate failed');
  const validation = {pixel_checks:pixelChecks, probes, decoded_ssim:{normalized_to:'limited-range yuv420p',frames:scores.length, min:Math.min(...scores), mean:scores.reduce((a,b) => a+b,0)/scores.length}};
  const median = values => [...values].sort((a,b) => a-b)[Math.floor(values.length / 2)];
  const medians = {rust-motion:median(samples.rust-motion), remotion:median(samples.remotion)};
  const report = {scene:'32 opaque spring-animated rectangles, no fonts/media/effects', width:1280, height:720, fps:30, frames:180, concurrency:4, codec:'h264', crf:18, preset:'fast', imageFormat, repeats, platform:os.platform(), arch:os.arch(), cpu:os.cpus()[0].model, node:process.version, chrome:execFileSync(browserExecutable,['--version'],{encoding:'utf8'}).trim(), validation, ffmpeg_shared:execFileSync(ffmpeg,['-version'],{encoding:'utf8', env:nativeEnv}).split('\n')[0], bundle_ms:bundleMs, browser_ms:browserMs, warmup, samples_ms:samples, native_render_ms:nativeRenderMs, host_load_before_samples:hostLoad, median_ms:medians, speedup:medians.remotion/medians.rust-motion, scope:'warm render plus encoding; reused Chrome; native includes process startup; bundling/browser startup excluded; PNG pixels and all decoded MP4 frames verified'};
  writeFileSync(path.join(root,`benchmarks/render-${imageFormat}.json`), JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report, null, 2));
  if (report.speedup <= 1) throw new Error('Performance gate failed: Rust Motion did not beat Remotion');
} finally {
  await browser.close({silent:true});
}
