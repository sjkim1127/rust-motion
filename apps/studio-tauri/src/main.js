import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { join, tempDir } from '@tauri-apps/api/path';
import { open, save } from '@tauri-apps/plugin-dialog';
import './style.css';

const app = document.querySelector('#app');
app.innerHTML = `
  <main class="studio">
    <header><h1>Dioxuscut Studio</h1><span id="backend">connecting…</span></header>
    <section class="toolbar">
      <input id="project-path" value="/tmp/dioxuscut-project.json" aria-label="project path" />
      <button id="load-project">Open project</button>
      <button id="save-project">Save project</button>
      <button id="submit-render">Queue render</button>
      <button id="cancel-render" disabled>Cancel</button>
      <span id="message"></span>
    </section>
    <section class="preview"><canvas id="preview-canvas"></canvas></section>
    <section class="timeline">
      <button id="play-toggle">Pause</button>
      <input id="timeline-slider" type="range" min="0" max="149" value="0" aria-label="timeline frame" />
      <span id="timeline-frame">0 / 149</span>
    </section>
    <section class="timeline-editor" aria-label="Project timeline editor">
      <div class="timeline-editor-header">
        <div class="timeline-title">
          <h2>Timeline</h2>
          <span id="timeline-summary">No tracks</span>
        </div>
        <div class="timeline-actions">
          <button id="add-track" type="button">+ Track</button>
          <button id="add-clip" type="button">+ Clip</button>
          <button id="add-time-event" type="button">+ Event</button>
          <label class="snap-control"><input id="timeline-snap" type="checkbox" checked /> Snap</label>
          <button id="zoom-out" type="button" aria-label="Zoom out">−</button>
          <span id="zoom-level">4 px/frame</span>
          <button id="zoom-in" type="button" aria-label="Zoom in">+</button>
        </div>
      </div>
      <div id="timeline-scroll" class="timeline-scroll" aria-label="Timeline tracks">
        <div id="timeline-canvas" class="timeline-canvas"></div>
      </div>
      <div id="clip-inspector" class="clip-inspector" hidden>
        <strong id="selected-clip-title">Selected clip</strong>
        <label>Composition <input id="clip-composition" type="text" /></label>
        <label>Start frame <input id="clip-start" type="number" min="0" step="1" /></label>
        <label>Duration <input id="clip-duration" type="number" min="1" step="1" /></label>
        <button id="duplicate-clip" type="button">Duplicate</button>
        <button id="remove-clip" type="button" class="danger-button">Delete</button>
      </div>
      <div id="time-event-inspector" class="clip-inspector" hidden>
        <strong id="selected-time-event-title">Selected event</strong>
        <label>Name <input id="time-event-name" type="text" /></label>
        <label>Frame <input id="time-event-frame" type="number" min="0" step="1" /></label>
        <button id="remove-time-event" type="button" class="danger-button">Delete</button>
        <small>Alt/Option-drag moves only this event; normal drag ripples later events.</small>
      </div>
      <div class="voice-over-controls">
        <label for="voice-over-asset">Voice-over</label>
        <select id="voice-over-asset" aria-label="Voice-over asset"></select>
        <button id="import-voice-over" type="button">Import audio</button>
        <audio id="voice-over-player" controls preload="metadata" hidden></audio>
      </div>
    </section>
    <section class="job-queue"><h2>Render queue</h2><div id="job-list">No jobs</div></section>
    <footer><span id="frame">frame 0</span><span id="protocol">worker protocol…</span><span id="job">job store…</span></footer>
  </main>`;

const canvas = document.querySelector('#preview-canvas');
const voiceOverPlayer = document.querySelector('#voice-over-player');
voiceOverPlayer.addEventListener('loadedmetadata', () => {
  const pendingSeek = Number(voiceOverPlayer.dataset.pendingSeek);
  if (!Number.isFinite(pendingSeek)) return;
  try {
    voiceOverPlayer.currentTime = Math.min(
      pendingSeek,
      Number.isFinite(voiceOverPlayer.duration) ? voiceOverPlayer.duration : pendingSeek,
    );
  } catch { /* The media element can apply this seek after its first frame loads. */ }
  delete voiceOverPlayer.dataset.pendingSeek;
});
let THREE;
let renderer;
let scene;
let camera;
let cube;
let threeReady;
async function ensureThree() {
  if (threeReady) return threeReady;
  threeReady = import('three').then((module) => {
    THREE = module;
    // Keep rendered pixels available across awaited clip callbacks and the
    // worker's subsequent canvas readback.
    renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      alpha: true,
      preserveDrawingBuffer: true,
    });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.setClearColor(0x0b1020);
    scene = new THREE.Scene();
    camera = new THREE.PerspectiveCamera(45, 1, 0.1, 100);
    camera.position.z = 3;
    scene.add(new THREE.HemisphereLight(0x9bbcff, 0x182033, 2));
    cube = new THREE.Mesh(
      new THREE.BoxGeometry(1, 1, 1),
      new THREE.MeshStandardMaterial({ color: 0x6c63ff, roughness: 0.28, metalness: 0.35 }),
    );
    scene.add(cube);
  });
  return threeReady;
}

// Browser compositions can replace the demo scene without changing the Rust
// protocol. Adapters may register Three.js, R3F, or another WebGL renderer.
const compositions = new Map();
const threeCompositions = new Map();
const remotionSpringDurationCache = new Map();

// Remotion 4.0.495-compatible default spring calculation used by the
// browser parity composition below. Keep this local to the browser adapter so
// the worker and Studio share the same deterministic frame semantics.
function remotionSpringCalculation(frame, fps, config = {}) {
  const damping = config.damping ?? 10;
  const mass = config.mass ?? 1;
  const stiffness = config.stiffness ?? 100;
  let current = 0;
  let velocity = 0;
  let lastTimestamp = 0;
  const frameClamped = Math.max(0, frame);
  const lastFrame = Math.floor(frameClamped);
  const unevenRest = frameClamped % 1;
  for (let f = 0; f <= lastFrame; f += 1) {
    const now = (f + (f === lastFrame ? unevenRest : 0)) / fps * 1000;
    const deltaTime = Math.min(now - lastTimestamp, 64);
    const c = damping;
    const m = mass;
    const k = stiffness;
    const x0 = 1 - current;
    const v0 = -velocity;
    const zeta = c / (2 * Math.sqrt(k * m));
    const omega0 = Math.sqrt(k / m);
    const omega1 = omega0 * Math.sqrt(Math.max(0, 1 - zeta ** 2));
    const t = deltaTime / 1000;
    const sin1 = Math.sin(omega1 * t);
    const cos1 = Math.cos(omega1 * t);
    const envelope = Math.exp(-zeta * omega0 * t);
    const fragment = envelope * (omega1 === 0
      ? x0 + (v0 + zeta * omega0 * x0) * t
      : sin1 * ((v0 + zeta * omega0 * x0) / omega1) + x0 * cos1);
    const underDampedPosition = 1 - fragment;
    const underDampedVelocity = zeta * omega0 * fragment - envelope * (
      cos1 * (v0 + zeta * omega0 * x0) - omega1 * x0 * sin1);
    const criticallyDampedEnvelope = Math.exp(-omega0 * t);
    const criticallyDampedPosition = 1 - criticallyDampedEnvelope *
      (x0 + (v0 + omega0 * x0) * t);
    const criticallyDampedVelocity = criticallyDampedEnvelope *
      (v0 * (t * omega0 - 1) + t * x0 * omega0 * omega0);
    current = zeta < 1 ? underDampedPosition : criticallyDampedPosition;
    velocity = zeta < 1 ? underDampedVelocity : criticallyDampedVelocity;
    lastTimestamp = now;
  }
  return current;
}

function remotionMeasureSpring(fps, config = {}, threshold = 0.005) {
  let frame = 0;
  let value = remotionSpringCalculation(frame, fps, config);
  while (Math.abs(value - 1) >= threshold) {
    frame += 1;
    value = remotionSpringCalculation(frame, fps, config);
  }
  let finishedFrame = frame;
  for (let i = 0; i < 20; i += 1) {
    frame += 1;
    value = remotionSpringCalculation(frame, fps, config);
    if (Math.abs(value - 1) >= threshold) {
      i = 0;
      finishedFrame = frame + 1;
    }
  }
  return finishedFrame;
}

function remotionSpring({ frame, fps, durationInFrames, delay = 0 }) {
  if (!remotionSpringDurationCache.has(fps)) {
    remotionSpringDurationCache.set(fps, remotionMeasureSpring(fps));
  }
  const naturalDuration = remotionSpringDurationCache.get(fps);
  const delayed = frame - delay;
  if (delayed > durationInFrames) return 1;
  const adjusted = delayed / (durationInFrames / naturalDuration);
  return remotionSpringCalculation(adjusted, fps);
}
const preloadedAssets = new Map();
const preloadedSources = new Map();
const imageDimensionsCache = new Map();
const videoMetadataCache = new Map();
const webmMetadataCache = new Map();
const webmClusterCache = new Map();
let webmClusterCacheBytes = 0;
const WEBM_CLUSTER_CACHE_MAX_BYTES = 64 * 1024 * 1024;
const audioDurationCache = new Map();
const audioDataCache = new Map();
const videoTextureCache = new Map();
const renderGates = new Map();
let lottieAdapter = null;
const lottieInstances = new WeakMap();
let lottieModulePromise;
let nextRenderGate = 1;
let renderGateError = null;

function delayRender(reason = 'render gate') {
  const handle = nextRenderGate++;
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  renderGates.set(handle, { promise, reason, resolve, reject });
  return handle;
}

function continueRender(handle) {
  const gate = renderGates.get(handle);
  if (!gate) return;
  renderGates.delete(handle);
  gate.resolve();
}

function cancelRender(handle, reason = 'render cancelled') {
  if (!renderGates.has(handle)) return;
  renderGates.delete(handle);
  renderGateError = new Error(String(reason));
}

// Hook-shaped aliases for compositions ported from Remotion. The worker is
// not React-driven, so these return the stable render-gate protocol helpers.
export function useDelayRender() {
  return { delayRender, continueRender, cancelRender };
}

// Minimal browser equivalent of Remotion's useBufferState. Media adapters can
// pause frame capture while a seek or decoder warm-up is pending, then release
// exactly the handle they acquired.
export function useBufferState() {
  return {
    delayPlayback() {
      const handle = delayRender('media buffering');
      return { unblock: () => continueRender(handle) };
    },
  };
}

// Browser pixel density for canvas/WebGL adapters. Headless workers use their
// configured device scale factor, keeping captures deterministic.
export function usePixelDensity() {
  return window.devicePixelRatio || 1;
}

async function waitForRenderGates() {
  if (renderGateError) throw renderGateError;
  while (renderGates.size > 0) {
    await Promise.all([...renderGates.values()].map((gate) => gate.promise));
    if (renderGateError) throw renderGateError;
  }
}
async function preloadAssets(assets = []) {
  await Promise.all(assets.map(async (source) => {
    if (preloadedAssets.has(source)) return preloadedAssets.get(source);
    const extension = source.split(/[?#]/, 1)[0].split('.').pop()?.toLowerCase();
    let task;
    if (['ttf', 'otf', 'woff', 'woff2'].includes(extension)) {
      task = new FontFace(`dioxuscut-${preloadedAssets.size}`, `url(${source})`).load();
      task = task.then((font) => { document.fonts.add(font); return font; });
    } else if (extension === 'json') {
      task = fetch(source).then((response) => {
        if (!response.ok) throw new Error(`failed to preload asset: ${source}`);
        return response.json();
      });
    } else if (['mp4', 'webm', 'mov', 'm4v'].includes(extension)) {
      task = new Promise((resolve, reject) => {
        const video = document.createElement('video');
        video.preload = 'metadata';
        video.onloadedmetadata = () => resolve(video);
        video.onerror = () => reject(new Error(`failed to preload asset: ${source}`));
        video.src = source;
      });
    } else {
      task = new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => image.decode().then(() => resolve(image), () => resolve(image));
        image.onerror = () => reject(new Error(`failed to preload asset: ${source}`));
        image.src = source;
      });
    }
    task = task.catch((error) => {
      preloadedAssets.delete(source);
      throw error;
    });
    preloadedAssets.set(source, task);
    return task;
  }));
}

export function registerComposition(id, render) {
  if (typeof id !== 'string' || !id || typeof render !== 'function') {
    throw new TypeError('registerComposition expects a non-empty id and render function');
  }
  compositions.set(id, render);
}

registerComposition('SpringRects', async ({ frame: nextFrame, fps, width = 1280, height = 720 }) => {
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('SpringRects requires a 2D canvas context');
  const scaleX = width / 1280;
  const scaleY = height / 720;
  context.fillStyle = 'rgb(15,23,42)';
  context.fillRect(0, 0, width, height);
  for (let index = 0; index < 32; index += 1) {
    const progress = remotionSpring({
      frame: nextFrame % 60,
      fps,
      durationInFrames: 24,
      delay: (index % 8) * 2,
    });
    const left = Math.round((60 + (index % 8) * 145 + progress * 40) * scaleX);
    const top = (80 + Math.floor(index / 8) * 140) * scaleY;
    context.fillStyle = `rgb(${80 + index * 4},160,220)`;
    context.fillRect(left, top, 64 * scaleX, 64 * scaleY);
  }
});

/**
 * Register a reusable Three.js composition with an explicit scene lifecycle.
 *
 * `setup` runs once per browser worker and may return `{scene, camera}` (or
 * any additional application state). `render` runs for every requested frame.
 * Keeping this contract outside the frame protocol lets the same scene module
 * run in Studio, the headless worker, and a future R3F adapter.
 */
export function registerThreeComposition(id, { setup, render, dispose } = {}) {
  if (typeof id !== 'string' || !id || typeof setup !== 'function' || typeof render !== 'function') {
    throw new TypeError('registerThreeComposition expects id, setup(), and render()');
  }
  const previous = threeCompositions.get(id);
  previous?.dispose?.();
  const entry = { setup, render, dispose, instance: null };
  threeCompositions.set(id, entry);
  registerComposition(id, async (context) => {
    if (!entry.instance) {
      await ensureThree();
      entry.instance = await setup({ THREE, renderer, canvas, ...context });
    }
    return render({
      THREE,
      renderer,
      canvas,
      ...context,
      ...(entry.instance && typeof entry.instance === 'object' ? entry.instance : {}),
    });
  });
}

registerThreeComposition('NamedTimeEventsDemo', {
  setup({ THREE }) {
    const scene = new THREE.Scene();
    scene.background = new THREE.Color('#0f172a');
    const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10);
    camera.position.z = 2;
    camera.lookAt(0, 0, 0);

    const rail = new THREE.Mesh(
      new THREE.PlaneGeometry(1.5, 0.16),
      new THREE.MeshBasicMaterial({ color: '#1e293b' }),
    );
    const fill = new THREE.Mesh(
      new THREE.PlaneGeometry(1.5, 0.16),
      new THREE.MeshBasicMaterial({ color: '#f9733e' }),
    );
    scene.add(rail, fill);
    return { scene, camera, fill };
  },
  render({ frame: nextFrame, width = 1280, height = 720, scene, camera, fill, renderer }) {
    const startFrame = waitUntil('voice_start');
    const duration = getTimeEventDuration('voice_start', 'voice_end');
    const progress = Math.max(0, Math.min(1,
      (nextFrame - startFrame) / Math.max(1, duration)));
    fill.scale.x = progress;
    fill.position.x = -0.75 * (1 - progress);
    renderer.setSize(width, height, false);
    camera.left = -Math.max(1, width / height) / 2;
    camera.right = Math.max(1, width / height) / 2;
    camera.top = 0.5;
    camera.bottom = -0.5;
    camera.updateProjectionMatrix();
    renderer.render(scene, camera);
  },
});

export function unregisterThreeComposition(id) {
  const entry = threeCompositions.get(id);
  if (!entry) return false;
  entry.dispose?.(entry.instance);
  entry.instance?.renderer?.dispose?.();
  entry.instance?.scene?.traverse?.((object) => {
    object.geometry?.dispose?.();
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
      material?.dispose?.();
    }
  });
  threeCompositions.delete(id);
  compositions.delete(id);
  return true;
}

// Browser equivalent of @remotion/media-utils/getImageDimensions. Dimensions
// are cached independently from decoded assets so layout probes do not force a
// second request or decode in a composition.
export async function getImageDimensions(source) {
  if (typeof source !== 'string' || !source) throw new TypeError('getImageDimensions expects a source URL');
  if (imageDimensionsCache.has(source)) return imageDimensionsCache.get(source);
  const dimensions = new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve({ width: image.naturalWidth, height: image.naturalHeight });
    image.onerror = () => reject(new Error(`failed to load image dimensions: ${source}`));
    image.src = source;
  }).catch((error) => {
    imageDimensionsCache.delete(source);
    throw error;
  });
  imageDimensionsCache.set(source, dimensions);
  return dimensions;
}

// Browser equivalent of @remotion/media-utils/getVideoMetadata.
export async function getVideoMetadata(source) {
  if (typeof source !== 'string' || !source) throw new TypeError('getVideoMetadata expects a source URL');
  if (videoMetadataCache.has(source)) return videoMetadataCache.get(source);
  const metadata = new Promise((resolve, reject) => {
    const video = document.createElement('video');
    const cleanup = () => { video.removeAttribute('src'); video.load(); };
    video.preload = 'metadata';
    video.onloadedmetadata = () => {
      if (!video.videoWidth || !video.videoHeight || !Number.isFinite(video.duration)) {
        reject(new Error(`unable to determine video metadata: ${source}`));
        cleanup();
        return;
      }
      resolve({
        durationInSeconds: video.duration,
        width: video.videoWidth,
        height: video.videoHeight,
        aspectRatio: video.videoWidth / video.videoHeight,
        isRemote: /^https?:\/\//i.test(source),
      });
      cleanup();
    };
    video.onerror = () => { reject(new Error(`failed to load video metadata: ${source}`)); cleanup(); };
    video.src = source;
  }).catch((error) => {
    videoMetadataCache.delete(source);
    throw error;
  });
  videoMetadataCache.set(source, metadata);
  return metadata;
}

// Metadata-first browser facade corresponding to @remotion/media-parser's
// parseMedia(). It reuses the existing cached probes and intentionally does
// not download sample payloads; callers that need samples use getAudioData or
// the video texture adapters.
export async function parseMedia({
  src,
  fields,
  onDimensions,
  onDurationInSeconds,
  onFps,
  onVideoCodec,
  onAudioCodec,
  onVideoTrack,
  onAudioTrack,
  onKeyframes,
  onSampleRate,
  onNumberOfAudioChannels,
  onContainer,
  onTracks,
  onParseProgress,
} = {}) {
  if (typeof src !== 'string' || !src) throw new TypeError('parseMedia expects {src}');
  for (const [name, callback] of Object.entries({
    onDimensions, onDurationInSeconds, onFps, onVideoCodec, onAudioCodec,
    onVideoTrack, onAudioTrack,
    onKeyframes,
    onSampleRate, onNumberOfAudioChannels, onContainer, onTracks, onParseProgress,
  })) {
    if (callback !== undefined && typeof callback !== 'function') {
      throw new TypeError(`parseMedia expects ${name} to be a function`);
    }
  }
  // This metadata facade does not yet expose container byte counts. Report the
  // same progress shape as @remotion/media-parser with an unknown total.
  await onParseProgress?.({ bytes: 0, percentage: 0, totalBytes: null });
  const selectFields = (result) => {
    if (fields === undefined || fields === null) return result;
    const requested = Array.isArray(fields)
      ? fields
      : typeof fields === 'object'
        ? Object.entries(fields).filter(([, enabled]) => enabled).map(([field]) => field)
        : null;
    if (!requested || requested.length === 0) return result;
    return Object.fromEntries(requested.filter((field) => field in result).map((field) => [field, result[field]]));
  };
  const [container, webmMetadata] = await Promise.all([
    parseIsoBmffMovieHeader(src).catch(() => null),
    parseWebmHeader(src).catch(() => null),
  ]);
  const webm = Boolean(webmMetadata);
  const wav = await parseWavMetadata(src).catch(() => null);
  if (wav) {
    await onDimensions?.(null);
    await onDurationInSeconds?.(wav.durationInSeconds);
    await onSampleRate?.(wav.sampleRate);
    await onNumberOfAudioChannels?.(wav.audioTracks[0]?.channels ?? null);
    await onAudioCodec?.(wav.audioFormat === 1 ? 'pcm' : null);
    await onContainer?.('wav');
    await onTracks?.([]);
    await onParseProgress?.({ bytes: 0, percentage: 1, totalBytes: null });
    return selectFields({ ...wav, container: 'wav', tracks: [] });
  }
  const [video, image, audioDuration] = await Promise.all([
    getVideoMetadata(src).catch(() => null),
    getImageDimensions(src).catch(() => null),
    getAudioDurationInSeconds(src).catch(() => null),
  ]);
  if (!video && !image && audioDuration === null && container?.durationInSeconds == null) {
    throw new Error(`unable to parse media metadata: ${src}`);
  }
  const webmTrack = webm && video ? {
    type: 'video',
    width: video.width,
    height: video.height,
    fps: video.fps,
    durationInSeconds: video.durationInSeconds,
    codec: webmCodec(webmMetadata.videoCodec),
    trackNumber: webmMetadata.videoTrackNumber,
  } : null;
  const webmAudioTrack = webm && webmMetadata.audioTrackNumber !== null ? {
    type: 'audio',
    trackNumber: webmMetadata.audioTrackNumber,
    codec: webmAudioCodec(webmMetadata.audioCodec),
    sampleRate: webmMetadata.audioSampleRate,
    numberOfChannels: webmMetadata.audioChannels,
  } : null;
  const videoTrack = container?.tracks?.find((track) => track.type === 'video') ?? webmTrack;
  const audioTrack = container?.tracks?.find((track) => track.type === 'audio') ?? webmAudioTrack;
  const dimensions = video
    ? { width: video.width, height: video.height }
    : videoTrack && videoTrack.width && videoTrack.height
      ? { width: videoTrack.width, height: videoTrack.height }
      : image;
  const durationInSeconds = video?.durationInSeconds ?? audioDuration ?? container?.durationInSeconds
    ?? webmMetadata?.durationInSeconds ?? 0;
  const fps = videoTrack?.fps ?? null;
  const videoCodec = videoTrack?.codecConfig
    ? makeIsoBmffWebCodecsConfig(videoTrack).codec
    : videoTrack?.codec ?? null;
  const audioCodec = audioTrack?.codecConfig
    ? makeIsoBmffWebCodecsConfig(audioTrack).codec
    : audioTrack?.codec ?? null;
  await onDimensions?.(dimensions);
  await onDurationInSeconds?.(durationInSeconds);
  if (fps !== null) await onFps?.(fps);
  if (videoCodec !== null) await onVideoCodec?.(videoCodec);
  if (audioCodec !== null) await onAudioCodec?.(audioCodec);
  if (audioTrack?.sampleRate != null) await onSampleRate?.(audioTrack.sampleRate);
  if (audioTrack?.numberOfChannels != null) await onNumberOfAudioChannels?.(audioTrack.numberOfChannels);
  if (container?.container != null) await onContainer?.(container.container);
  else if (webm) await onContainer?.('webm');
  if (videoTrack) {
    await onVideoTrack?.({
      ...videoTrack,
      width: video?.width ?? videoTrack.width ?? null,
      height: video?.height ?? videoTrack.height ?? null,
      codec: videoCodec,
    });
  }
  if (audioTrack) await onAudioTrack?.({ ...audioTrack, codec: audioCodec });
  if (videoTrack?.sampleTables) {
    const tables = videoTrack.sampleTables;
    const keyframeNumbers = tables.keyframes?.length
      ? tables.keyframes
      : (tables.sampleRanges ?? [])
        .filter((sample) => sample.keyframe)
        .map((sample) => sample.sampleIndex + 1);
    const keyframes = keyframeNumbers
      .map((sampleNumber) => tables.sampleRanges?.[sampleNumber - 1])
      .filter(Boolean)
      .map((sample) => ({
        positionInBytes: sample.offset,
        sizeInBytes: sample.size,
        presentationTimeInSeconds: sample.presentationTimestamp ?? sample.timestamp ?? 0,
        decodingTimeInSeconds: sample.timestamp ?? 0,
        trackId: (container?.tracks?.indexOf(videoTrack) ?? 0) + 1,
      }));
    await onKeyframes?.(keyframes);
  } else if (webmMetadata?.cues?.length) {
    await onKeyframes?.(webmMetadata.cues.map((cue) => ({
      positionInBytes: cue.clusterPosition,
      sizeInBytes: 0,
      presentationTimeInSeconds: cue.timeInSeconds ?? 0,
      decodingTimeInSeconds: cue.timeInSeconds ?? 0,
      trackId: cue.trackNumber ?? 1,
    })));
  }
  const tracks = container?.tracks ?? [webmTrack, webmAudioTrack].filter(Boolean);
  await onTracks?.(tracks);
  await onParseProgress?.({ bytes: 0, percentage: 1, totalBytes: null });
  const resolvedWidth = video?.width ?? videoTrack?.width;
  const resolvedHeight = video?.height ?? videoTrack?.height;
  const result = {
    durationInSeconds,
    dimensions,
    videoTracks: resolvedWidth && resolvedHeight ? [{
      width: resolvedWidth,
      height: resolvedHeight,
      aspectRatio: resolvedWidth / resolvedHeight,
    }] : [],
    videoCodec,
    audioCodec,
    audioTracks: audioDuration !== null ? [{ durationInSeconds: audioDuration }] : [],
    container: container?.container ?? (webm ? 'webm' : null),
    tracks,
    keyframes: webmMetadata?.cues ?? undefined,
    isRemote: /^https?:\/\//i.test(src),
  };
  return selectFields(result);
}

async function parseWebmHeader(source) {
  if (webmMetadataCache.has(source)) return webmMetadataCache.get(source);
  const metadata = parseWebmHeaderUncached(source).catch((error) => {
    webmMetadataCache.delete(source);
    throw error;
  });
  webmMetadataCache.set(source, metadata);
  return metadata;
}

async function parseWebmHeaderUncached(source) {
  const signature = await readMediaRange(source, 0, 4);
  if (signature.length !== 4 || signature[0] !== 0x1a || signature[1] !== 0x45
    || signature[2] !== 0xdf || signature[3] !== 0xa3) return null;
  const response = await fetch(source);
  if (!response.ok) throw new Error(`failed to read WebM header: ${response.status}`);
  const bytes = await readBoundedResponse(response, 16 * 1024 * 1024);
  const elements = [];
  const readVint = (offset, forSize = false) => {
    if (offset >= bytes.length) return null;
    const first = bytes[offset];
    let mask = 0x80;
    let width = 1;
    while (width <= 8 && !(first & mask)) { mask >>= 1; width += 1; }
    if (width > 8 || offset + width > bytes.length) return null;
    let value = first & (mask - 1);
    for (let index = 1; index < width; index += 1) value = value * 256 + bytes[offset + index];
    return { width, value: forSize && value === (2 ** (7 * width)) - 1 ? null : value };
  };
  const readId = (offset) => {
    if (offset >= bytes.length) return null;
    const first = bytes[offset];
    let mask = 0x80;
    let width = 1;
    while (width <= 4 && !(first & mask)) { mask >>= 1; width += 1; }
    if (width > 4 || offset + width > bytes.length) return null;
    let value = 0;
    for (let index = 0; index < width; index += 1) value = value * 256 + bytes[offset + index];
    return { width, value };
  };
  const masters = new Set([
    0x1a45dfa3, 0x18538067, 0x1549a966, 0x1654ae6b, 0xae, 0xe0,
    0x1c53bb6b, 0xbb, 0xb7, 0xe1,
  ]);
  const parseRange = (rangeStart, rangeEnd) => {
    let offset = rangeStart;
    while (offset + 2 <= rangeEnd) {
      const idInfo = readId(offset);
      if (!idInfo) break;
      const sizeInfo = readVint(offset + idInfo.width, true);
      if (!sizeInfo || sizeInfo.value === null) break;
      const start = offset + idInfo.width + sizeInfo.width;
      const end = Math.min(rangeEnd, start + sizeInfo.value);
      if (end <= offset) break;
      elements.push({ id: idInfo.value, start, end });
      if (masters.has(idInfo.value)) parseRange(start, end);
      offset = end;
    }
  };
  parseRange(0, bytes.length);
  const integer = ({ start, end }) => {
    let value = 0;
    for (let index = start; index < end; index += 1) value = value * 256 + bytes[index];
    return value;
  };
  const float = ({ start, end }) => {
    const view = new DataView(bytes.buffer, bytes.byteOffset + start, end - start);
    return end - start === 4 ? view.getFloat32(0, false) : view.getFloat64(0, false);
  };
  const find = (id, start = 0, end = bytes.length) => elements.find((element) =>
    element.id === id && element.start >= start && element.end <= end);
  const findAll = (id, start = 0, end = bytes.length) => elements.filter((element) =>
    element.id === id && element.start >= start && element.end <= end);
  const segment = find(0x18538067);
  if (!segment) return null;
  const info = find(0x1549a966, segment.start, segment.end);
  const scale = info ? find(0x2ad7b1, info.start, info.end) : null;
  const duration = info ? find(0x4489, info.start, info.end) : null;
  const tracks = find(0x1654ae6b, segment.start, segment.end);
  const entries = tracks ? findAll(0xae, tracks.start, tracks.end) : [];
  const trackInfo = entries.map((entry) => {
    const type = find(0x83, entry.start, entry.end);
    const number = find(0xd7, entry.start, entry.end);
    const codec = find(0x86, entry.start, entry.end);
    const video = find(0xe0, entry.start, entry.end);
    const audio = find(0xe1, entry.start, entry.end);
    const samplingFrequency = audio ? find(0xb5, audio.start, audio.end) : null;
    const channels = audio ? find(0x9f, audio.start, audio.end) : null;
    return {
      type: type ? integer(type) : null,
      trackNumber: number ? integer(number) : null,
      codec: codec ? new TextDecoder().decode(bytes.subarray(codec.start, codec.end)) : null,
      video, audio,
      sampleRate: samplingFrequency ? float(samplingFrequency) : null,
      channels: channels ? integer(channels) : null,
    };
  });
  const videoInfo = trackInfo.find((track) => track.type === 1) ?? null;
  const audioInfo = trackInfo.find((track) => track.type === 2) ?? null;
  const entry = videoInfo ? entries[trackInfo.indexOf(videoInfo)] : null;
  const trackType = videoInfo?.type;
  const trackNumber = videoInfo?.trackNumber;
  const codec = videoInfo?.codec;
  const video = videoInfo?.video;
  const cues = findAll(0xbb, segment.start, segment.end).map((cuePoint) => {
    const cueTime = find(0xb3, cuePoint.start, cuePoint.end);
    const positions = findAll(0xb7, cuePoint.start, cuePoint.end);
    return positions.map((position) => {
      const track = find(0xf7, position.start, position.end);
      const cluster = find(0xf1, position.start, position.end);
      return {
        timeInSeconds: cueTime && scale ? integer(cueTime) * integer(scale) / 1e9 : null,
        trackNumber: track ? integer(track) : null,
        clusterPosition: cluster ? segment.start + integer(cluster) : null,
      };
    });
  }).flat().filter((cue) => cue.clusterPosition !== null);
  const width = video ? find(0xb0, video.start, video.end) : null;
  const height = video ? find(0xba, video.start, video.end) : null;
  return {
    container: 'webm',
    durationInSeconds: duration && scale ? float(duration) * integer(scale) / 1e9 : null,
    timecodeScale: scale ? integer(scale) : 1_000_000,
    videoCodec: codec,
    videoTrackNumber: trackNumber,
    videoWidth: width ? integer(width) : null,
    videoHeight: height ? integer(height) : null,
    videoType: trackType,
    audioCodec: audioInfo?.codec ?? null,
    audioTrackNumber: audioInfo?.trackNumber ?? null,
    audioSampleRate: audioInfo?.sampleRate ?? null,
    audioChannels: audioInfo?.channels ?? null,
    cues,
  };
}

function webmCodec(codecId) {
  if (codecId === 'V_VP8') return 'vp8';
  if (codecId === 'V_VP9') return 'vp09.00.10.08';
  if (codecId === 'V_AV1') return 'av01.0.08M.08';
  return null;
}

function webmAudioCodec(codecId) {
  if (codecId === 'A_OPUS') return 'opus';
  if (codecId === 'A_VORBIS') return 'vorbis';
  return null;
}

async function readBoundedResponse(response, maxBytes) {
  const declaredLength = Number(response.headers.get('content-length'));
  if (Number.isFinite(declaredLength) && declaredLength > maxBytes) {
    throw new RangeError(`media response exceeds ${maxBytes} byte safety limit`);
  }
  if (!response.body) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.length > maxBytes) throw new RangeError(`media response exceeds ${maxBytes} byte safety limit`);
    return bytes;
  }
  const reader = response.body.getReader();
  const chunks = []; let total = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > maxBytes) throw new RangeError(`media response exceeds ${maxBytes} byte safety limit`);
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return bytes;
}

// Read VP8/VP9/AV1 SimpleBlock payloads from a WebM cluster. Ordinary blocks
// and fixed/Xiph/EBML-laced blocks are supported. Laced frames share the
// block timestamp because Matroska does not store per-frame timestamps there.
async function readWebmSamplesFromCue(source, trackNumber = 1, options = {}) {
  const metadata = options.metadata?.cues ? options.metadata : await parseWebmHeader(source);
  if (!metadata?.cues?.length) return [];
  const trackCues = metadata.cues.filter((cue) => cue.trackNumber === trackNumber || cue.trackNumber == null);
  const cues = trackCues.length ? trackCues : metadata.cues;
  const cueIndex = Math.max(0, Number(options.cueIndex ?? 0));
  const start = cues[cueIndex]?.clusterPosition;
  if (!Number.isSafeInteger(start)) return [];
  const cacheKey = `${source}|${trackNumber}|${cueIndex}`;
  if (options.cache !== false && webmClusterCache.has(cacheKey)) {
    const cached = webmClusterCache.get(cacheKey);
    webmClusterCache.delete(cacheKey);
    webmClusterCache.set(cacheKey, cached);
    return cached.samples;
  }
  const next = cues.slice(cueIndex + 1).find((cue) => cue.clusterPosition > start)?.clusterPosition;
  const requestedEnd = (next ?? start + 16 * 1024 * 1024);
  let response = await fetch(source, { headers: { Range: `bytes=${start}-${requestedEnd - 1}` } });
  if (response.status === 416) response = await fetch(source);
  if (!response.ok) throw new Error(`failed to read WebM cluster: ${response.status}`);
  const responseBytes = await readBoundedResponse(response, 16 * 1024 * 1024);
  const bytes = response.status === 200
    ? responseBytes.slice(start, Math.min(requestedEnd, responseBytes.length))
    : responseBytes;
  const samples = [];
  const readVint = (offset) => {
    const first = bytes[offset];
    if (first === undefined) return null;
    let mask = 0x80; let width = 1;
    while (width <= 8 && !(first & mask)) { mask >>= 1; width += 1; }
    if (width > 8 || offset + width > bytes.length) return null;
    let value = first & (mask - 1);
    for (let index = 1; index < width; index += 1) value = value * 256 + bytes[offset + index];
    return { width, value };
  };
  const readId = (offset) => {
    const first = bytes[offset];
    if (first === undefined) return null;
    let mask = 0x80; let width = 1;
    while (width <= 4 && !(first & mask)) { mask >>= 1; width += 1; }
    if (width > 4 || offset + width > bytes.length) return null;
    let value = 0;
    for (let index = 0; index < width; index += 1) value = value * 256 + bytes[offset + index];
    return { width, value };
  };
  const parseRange = (rangeStart, rangeEnd) => {
    let offset = rangeStart;
    while (offset + 2 <= rangeEnd) {
      const id = readId(offset); if (!id) break;
      const size = readVint(offset + id.width); if (!size) break;
      const payload = offset + id.width + size.width;
      const end = Math.min(rangeEnd, payload + size.value);
      if (end <= offset) break;
      if (id.value === 0x1f43b675) parseRange(payload, end);
      if (id.value === 0xa3 && end > payload + 4) {
      const track = readVint(payload);
      if (track && track.value === trackNumber) {
        const timecode = (bytes[payload + track.width] << 8) | bytes[payload + track.width + 1];
        const signedTimecode = timecode & 0x8000 ? timecode - 0x10000 : timecode;
        const flags = bytes[payload + track.width + 2];
        const blockDataStart = payload + track.width + 3;
        const lacing = flags & 0x06;
        let dataStart = blockDataStart;
        let frameSizes = [end - dataStart];
        if (lacing === 0x04 && dataStart < end) {
          const frameCount = bytes[dataStart] + 1;
          dataStart += 1;
          const payloadBytes = end - dataStart;
          if (frameCount > 0 && payloadBytes % frameCount === 0) {
            frameSizes = Array.from({ length: frameCount }, () => payloadBytes / frameCount);
          } else frameSizes = [];
        } else if ((lacing === 0x02 || lacing === 0x06) && dataStart < end) {
          const frameCount = bytes[dataStart] + 1;
          dataStart += 1;
          const sizes = [];
          if (lacing === 0x02) {
            for (let frameIndex = 0; frameIndex < frameCount - 1 && dataStart < end; frameIndex += 1) {
              let size = 0; let part;
              do { part = bytes[dataStart++]; size += part; } while (part === 255 && dataStart < end);
              sizes.push(size);
            }
          } else {
            const firstSize = readVint(dataStart);
            if (firstSize) {
              sizes.push(firstSize.value);
              dataStart += firstSize.width;
              for (let frameIndex = 1; frameIndex < frameCount - 1 && dataStart < end; frameIndex += 1) {
                const encoded = readVint(dataStart);
                if (!encoded) break;
                const bias = (2 ** (7 * encoded.width - 1)) - 1;
                sizes.push((sizes[sizes.length - 1] ?? 0) + encoded.value - bias);
                dataStart += encoded.width;
              }
            }
          }
          const remaining = end - dataStart - sizes.reduce((sum, size) => sum + size, 0);
          if (sizes.length === frameCount - 1 && remaining >= 0) frameSizes = [...sizes, remaining];
          else frameSizes = [];
        } else if (lacing !== 0) frameSizes = [];
        let frameOffset = dataStart;
        for (let frameIndex = 0; frameIndex < frameSizes.length; frameIndex += 1) {
          const frameSize = frameSizes[frameIndex];
          samples.push({
            timestamp: (cues[cueIndex].timeInSeconds ?? 0) + signedTimecode * metadata.timecodeScale / 1e9,
            keyframe: Boolean(flags & 0x80) && frameIndex === 0,
            offset: start + frameOffset,
            size: frameSize,
            data: bytes.slice(frameOffset, frameOffset + frameSize),
          });
          frameOffset += frameSize;
        }
      }
    }
      offset = end;
    }
  };
  parseRange(0, bytes.length);
  if (options.cache !== false) {
    const bytes = samples.reduce((total, sample) => total + sample.data.byteLength, 0);
    const previous = webmClusterCache.get(cacheKey);
    if (previous) webmClusterCacheBytes -= previous.bytes;
    webmClusterCache.delete(cacheKey);
    webmClusterCacheBytes += bytes;
    webmClusterCache.set(cacheKey, { samples, bytes });
    while (webmClusterCacheBytes > WEBM_CLUSTER_CACHE_MAX_BYTES && webmClusterCache.size > 1) {
      const oldestKey = webmClusterCache.keys().next().value;
      const oldest = webmClusterCache.get(oldestKey);
      webmClusterCacheBytes -= oldest.bytes;
      webmClusterCache.delete(oldestKey);
    }
  }
  return samples;
}

export async function readWebmSamples(source, trackNumber = 1, options = {}) {
  const metadata = options.metadata?.cues ? options.metadata : await parseWebmHeader(source);
  if (!metadata?.cues?.length) return [];
  const maxSamples = Number.isInteger(options.maxSamples) ? options.maxSamples : Infinity;
  if (maxSamples <= 0) return [];
  const trackCues = metadata.cues.filter((cue) => cue.trackNumber === trackNumber || cue.trackNumber == null);
  const matchingCues = trackCues.length ? trackCues : metadata.cues;
  const startCue = Math.max(0, Number(options.cueIndex ?? 0));
  const maxCues = Number.isInteger(options.maxCues) ? Math.max(1, options.maxCues) : 32;
  const cueIndexes = matchingCues.slice(startCue, startCue + maxCues).map((_, index) => startCue + index);
  const concurrency = Number.isInteger(options.concurrency) ? Math.max(1, options.concurrency) : 4;
  const results = new Array(cueIndexes.length);
  let next = 0;
  const worker = async () => {
    while (next < cueIndexes.length) {
      const resultIndex = next++;
      results[resultIndex] = await readWebmSamplesFromCue(source, trackNumber, {
        ...options,
        metadata,
        cueIndex: cueIndexes[resultIndex],
      });
    }
  };
  await Promise.all(Array.from({ length: Math.min(concurrency, cueIndexes.length) }, worker));
  return results.flat().slice(0, maxSamples);
}

export function createWebmEncodedVideoChunk(sample, duration = 0) {
  if (typeof EncodedVideoChunk === 'undefined') throw new Error('EncodedVideoChunk is not available in this runtime');
  return new EncodedVideoChunk({
    type: sample.keyframe ? 'key' : 'delta',
    timestamp: Math.round(sample.timestamp * 1_000_000),
    duration: duration > 0 ? Math.round(duration * 1_000_000) : undefined,
    data: sample.data,
  });
}

export function createWebmEncodedAudioChunk(sample, duration = 0) {
  if (typeof EncodedAudioChunk === 'undefined') throw new Error('EncodedAudioChunk is not available in this runtime');
  return new EncodedAudioChunk({
    type: 'key',
    timestamp: Math.round(sample.timestamp * 1_000_000),
    duration: duration > 0 ? Math.round(duration * 1_000_000) : undefined,
    data: sample.data,
  });
}

// Decode VP8/VP9/AV1 SimpleBlock samples through the same bounded lifecycle
// used by the ISO-BMFF backend. The codec string is supplied by the caller
// because Matroska CodecID does not contain the WebCodecs profile fields.
export async function decodeWebmVideo(source, {
  trackNumber, cueIndex = 0, maxSamples = 256, codec, options = {}, onFrame,
} = {}) {
  if (typeof VideoDecoder === 'undefined') throw new Error('VideoDecoder is not available in this runtime');
  const metadata = options.metadata ?? await parseWebmHeader(source);
  const resolvedTrackNumber = trackNumber ?? metadata?.videoTrackNumber ?? 1;
  const resolvedCodec = codec ?? webmCodec(metadata?.videoCodec);
  if (typeof resolvedCodec !== 'string' || !resolvedCodec) throw new TypeError('decodeWebmVideo requires a codec string');
  if (!Number.isInteger(maxSamples) || maxSamples <= 0) throw new RangeError('maxSamples must be a positive integer');
  if (onFrame !== undefined && typeof onFrame !== 'function') throw new TypeError('onFrame must be a function');
  const samples = (await readWebmSamples(source, resolvedTrackNumber, { ...options, metadata, cueIndex }))
    .slice(0, maxSamples);
  if (!samples.length) throw new RangeError('decodeWebmVideo found no samples in the requested cue');
  const frames = onFrame ? null : [];
  let failure;
  const decoder = new VideoDecoder({
    output: (frame) => { if (onFrame) onFrame(frame); else frames.push(frame); },
    error: (error) => { failure = error; },
  });
  const config = { codec: resolvedCodec };
  if (metadata.videoWidth && metadata.videoHeight) {
    config.codedWidth = metadata.videoWidth;
    config.codedHeight = metadata.videoHeight;
  }
  if (typeof VideoDecoder.isConfigSupported === 'function') {
    const support = await VideoDecoder.isConfigSupported(config);
    if (!support.supported) { decoder.close(); throw new Error(`WebCodecs does not support video codec: ${resolvedCodec}`); }
  }
  decoder.configure(config);
  try {
    for (const sample of samples) {
      if (failure) throw failure;
      decoder.decode(createWebmEncodedVideoChunk(sample, samples[1]?.timestamp - samples[0]?.timestamp || 1 / 24));
    }
    await decoder.flush();
    if (failure) throw failure;
    return onFrame ? null : frames;
  } catch (error) {
    for (const frame of frames ?? []) frame.close();
    throw error;
  } finally {
    if (decoder.state !== 'closed') decoder.close();
  }
}

export async function decodeWebmAudio(source, {
  trackNumber, cueIndex = 0, maxSamples = 256, codec, numberOfChannels, sampleRate,
  options = {}, onAudioData,
} = {}) {
  if (typeof AudioDecoder === 'undefined') throw new Error('AudioDecoder is not available in this runtime');
  if (!Number.isInteger(maxSamples) || maxSamples <= 0) throw new RangeError('maxSamples must be a positive integer');
  if (onAudioData !== undefined && typeof onAudioData !== 'function') throw new TypeError('onAudioData must be a function');
  const metadata = options.metadata ?? await parseWebmHeader(source);
  const resolvedTrackNumber = trackNumber ?? metadata?.audioTrackNumber ?? 1;
  const resolvedCodec = codec ?? webmAudioCodec(metadata?.audioCodec);
  const resolvedChannels = numberOfChannels ?? metadata?.audioChannels ?? 2;
  const resolvedSampleRate = sampleRate ?? metadata?.audioSampleRate ?? 48_000;
  if (typeof resolvedCodec !== 'string' || !resolvedCodec) throw new TypeError('decodeWebmAudio requires a codec string');
  const samples = (await readWebmSamples(source, resolvedTrackNumber, { ...options, metadata, cueIndex }))
    .slice(0, maxSamples);
  if (!samples.length) throw new RangeError('decodeWebmAudio found no samples in the requested cue');
  const chunks = onAudioData ? null : [];
  let failure;
  const decoder = new AudioDecoder({
    output: (audio) => { if (onAudioData) onAudioData(audio); else chunks.push(audio); },
    error: (error) => { failure = error; },
  });
  const config = { codec: resolvedCodec, sampleRate: resolvedSampleRate, numberOfChannels: resolvedChannels };
  if (typeof AudioDecoder.isConfigSupported === 'function') {
    const support = await AudioDecoder.isConfigSupported(config);
    if (!support.supported) { decoder.close(); throw new Error(`WebCodecs does not support audio codec: ${resolvedCodec}`); }
  }
  decoder.configure(config);
  try {
    const duration = samples[1]?.timestamp - samples[0]?.timestamp || 0.02;
    for (const sample of samples) {
      if (failure) throw failure;
      decoder.decode(createWebmEncodedAudioChunk(sample, duration));
    }
    await decoder.flush();
    if (failure) throw failure;
    return onAudioData ? null : chunks;
  } catch (error) {
    for (const chunk of chunks ?? []) chunk.close();
    throw error;
  } finally {
    if (decoder.state !== 'closed') decoder.close();
  }
}

// Browser counterpart of the native bounded range reader used by the
// parseMedia foundation. `endExclusive` follows the same half-open contract.
export async function readMediaRange(source, start, endExclusive, { requestInit } = {}) {
  if (typeof source !== 'string' || !source) throw new TypeError('readMediaRange expects a source URL');
  if (!Number.isInteger(start) || !Number.isInteger(endExclusive) || start < 0 || endExclusive < start) {
    throw new RangeError('readMediaRange expects a non-negative half-open range');
  }
  const length = endExclusive - start;
  if (length > 16 * 1024 * 1024) throw new RangeError('media range exceeds 16 MiB');
  if (length === 0) return new Uint8Array();
  const headers = new Headers(requestInit?.headers);
  headers.set('Range', `bytes=${start}-${endExclusive - 1}`);
  const response = await fetch(source, { ...requestInit, headers });
  if (!response.ok) throw new Error(`media range request failed: ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (response.status === 206 && bytes.length !== length) {
    throw new Error(`media range response length ${bytes.length} did not match requested length ${length}`);
  }
  if (response.status === 206) {
    const contentRange = response.headers.get('Content-Range') ?? '';
    const match = /^bytes\s+(\d+)-(\d+)\/(?:\d+|\*)$/i.exec(contentRange);
    if (!match || Number(match[1]) !== start || Number(match[2]) !== endExclusive - 1) {
      throw new Error(`media range response did not match requested range ${start}-${endExclusive - 1}`);
    }
  }
  if (response.status === 200) {
    if (start >= bytes.length) return new Uint8Array();
    return bytes.slice(start, Math.min(endExclusive, bytes.length));
  }
  return bytes;
}

function ascii(bytes, offset, length) {
  return String.fromCharCode(...bytes.subarray(offset, offset + length));
}

function uint32le(bytes, offset) {
  return (bytes[offset] | (bytes[offset + 1] << 8) | (bytes[offset + 2] << 16) | (bytes[offset + 3] << 24)) >>> 0;
}

// Incremental WAV probe: only RIFF headers and chunk headers are fetched.
// This mirrors the bounded-reader design of @remotion/media-parser without
// downloading PCM payloads merely to answer metadata queries.
export async function parseWavMetadata(source, { requestInit } = {}) {
  const header = await readMediaRange(source, 0, 12, { requestInit });
  if (header.length < 12 || ascii(header, 0, 4) !== 'RIFF' || ascii(header, 8, 4) !== 'WAVE') return null;
  let offset = 12;
  let format = null;
  let dataBytes = null;
  for (let chunk = 0; chunk < 4096; chunk += 1) {
    const chunkHeader = await readMediaRange(source, offset, offset + 8, { requestInit });
    if (chunkHeader.length < 8) break;
    const id = ascii(chunkHeader, 0, 4);
    const size = uint32le(chunkHeader, 4);
    if (!Number.isSafeInteger(size) || size < 0) break;
    if (id === 'fmt ' && size >= 16) {
      const bytes = await readMediaRange(source, offset + 8, offset + 24, { requestInit });
      format = {
        audioFormat: bytes[0] | (bytes[1] << 8),
        channels: bytes[2] | (bytes[3] << 8),
        sampleRate: uint32le(bytes, 4) >>> 0,
        blockAlign: bytes[12] | (bytes[13] << 8),
      };
    } else if (id === 'data') {
      dataBytes = size;
    }
    offset += 8 + size + (size & 1);
    if (format && dataBytes !== null) break;
  }
  if (!format || !format.channels || !format.sampleRate || !format.blockAlign || dataBytes === null) return null;
  const durationInSeconds = dataBytes / (format.sampleRate * format.blockAlign);
  return {
    durationInSeconds,
    audioTracks: [{ channels: format.channels, sampleRate: format.sampleRate }],
    dimensions: null,
    videoTracks: [],
    isRemote: /^https?:\/\//i.test(source),
    audioFormat: format.audioFormat,
  };
}

// Bounded ISO-BMFF structure probe. It discovers top-level boxes without
// fetching media payloads, which is useful for remote MP4/MOV assets and is
// the first stage of the native/browser container parser shared contract.
export async function probeIsoBmff(source, { requestInit, maxBytes = 16 * 1024 * 1024 } = {}) {
  if (!Number.isInteger(maxBytes) || maxBytes < 8 || maxBytes > 16 * 1024 * 1024) {
    throw new RangeError('probeIsoBmff maxBytes must be between 8 and 16 MiB');
  }
  const boxes = [];
  let offset = 0;
  while (offset + 8 <= maxBytes) {
    const head = await readMediaRange(source, offset, offset + 8, { requestInit });
    if (head.length < 8) break;
    const size32 = uint32be(head, 0);
    const type = ascii(head, 4, 4);
    let headerSize = 8;
    let size = size32;
    if (size32 === 1) {
      const extended = await readMediaRange(source, offset + 8, offset + 16, { requestInit });
      if (extended.length < 8) break;
      size = Number((BigInt(uint32be(extended, 0)) << 32n) | BigInt(uint32be(extended, 4)));
      headerSize = 16;
    } else if (size32 === 0) {
      break;
    }
    if (!Number.isSafeInteger(size) || size < headerSize || offset + size > maxBytes) break;
    boxes.push({ type, offset, size, headerSize, payloadOffset: offset + headerSize });
    offset += size;
  }
  if (!boxes.some(({ type }) => type === 'ftyp')) return null;
  return {
    container: 'iso-base-media',
    boxes,
    hasMovieHeader: boxes.some(({ type }) => type === 'moov'),
    isRemote: /^https?:\/\//i.test(source),
  };
}

function findIsoBox(bytes, wanted, start = 0, end = bytes.length) {
  let offset = start;
  while (offset + 8 <= end) {
    const size32 = uint32be(bytes, offset);
    const type = ascii(bytes, offset + 4, 4);
    const headerSize = size32 === 1 ? 16 : 8;
    const size = size32 === 1 && offset + 16 <= end
      ? Number((BigInt(uint32be(bytes, offset + 8)) << 32n) | BigInt(uint32be(bytes, offset + 12)))
      : size32;
    if (!Number.isSafeInteger(size) || size < headerSize || offset + size > end) break;
    if (type === wanted) return { offset, size, headerSize };
    offset += size;
  }
  return null;
}

function isoChildren(bytes, start, end) {
  const children = [];
  let offset = start;
  while (offset + 8 <= end) {
    const size32 = uint32be(bytes, offset);
    const headerSize = size32 === 1 ? 16 : 8;
    const size = size32 === 1 && offset + 16 <= end
      ? Number((BigInt(uint32be(bytes, offset + 8)) << 32n) | BigInt(uint32be(bytes, offset + 12)))
      : size32;
    if (!Number.isSafeInteger(size) || size < headerSize || offset + size > end) break;
    children.push({ type: ascii(bytes, offset + 4, 4), offset, size, headerSize });
    offset += size;
  }
  return children;
}

function isoPath(bytes, root, path) {
  let scope = [root];
  for (const type of path) {
    const next = [];
    for (const box of scope) {
      next.push(...isoChildren(bytes, box.offset + box.headerSize, box.offset + box.size)
        .filter((child) => child.type === type));
    }
    scope = next;
  }
  return scope[0] ?? null;
}

function findCodecConfig(bytes, root) {
  const types = new Set(['avcC', 'hvcC', 'av1C', 'esds']);
  const start = root.offset + root.headerSize;
  const end = root.offset + root.size;
  for (let offset = start; offset + 8 <= end; offset += 1) {
    const size = uint32be(bytes, offset);
    const type = ascii(bytes, offset + 4, 4);
    if (!types.has(type) || size < 8 || offset + size > end) continue;
    return { type, data: bytes.slice(offset + 8, offset + size) };
  }
  return null;
}

function parseAacAudioSpecificConfig(config) {
  if (!config || config.length < 2) return null;
  let asc = config;
  for (let index = 0; index + 2 < config.length; index += 1) {
    if (config[index] !== 0x05) continue;
    let cursor = index + 1;
    let length = 0;
    let lengthByte;
    do {
      if (cursor >= config.length) break;
      lengthByte = config[cursor++];
      length = (length << 7) | (lengthByte & 0x7f);
    } while (lengthByte & 0x80);
    if (length >= 2 && cursor + length <= config.length) {
      asc = config.slice(cursor, cursor + length);
      break;
    }
  }
  config = asc;
  const audioObjectType = (config[0] >> 3) & 0x1f;
  const frequencyIndex = ((config[0] & 0x07) << 1) | (config[1] >> 7);
  const frequencies = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050,
    16000, 12000, 11025, 8000, 7350];
  const sampleRate = frequencyIndex === 15 && config.length >= 5
    ? ((config[1] & 0x7f) << 17) | (config[2] << 9) | (config[3] << 1) | (config[4] >> 7)
    : frequencies[frequencyIndex];
  const channelConfiguration = (config[1] >> 3) & 0x0f;
  const channels = [0, 1, 2, 3, 4, 5, 6, 8][channelConfiguration] ?? null;
  return audioObjectType && sampleRate && channels ? { sampleRate, channels } : null;
}

// Read and decode only the movie header from an ISO-BMFF moov box. The box is
// bounded by the same 16 MiB cap as all other media range reads.
export async function parseIsoBmffMovieHeader(source, { requestInit, maxBytes = 16 * 1024 * 1024 } = {}) {
  const structure = await probeIsoBmff(source, { requestInit, maxBytes });
  const moov = structure?.boxes.find(({ type }) => type === 'moov');
  if (!moov || moov.size > maxBytes) return structure ? { ...structure, durationInSeconds: null } : null;
  const bytes = await readMediaRange(source, moov.offset, moov.offset + moov.size, { requestInit });
  const mvhd = findIsoBox(bytes, 'mvhd', moov.headerSize, bytes.length);
  if (!mvhd || mvhd.offset + mvhd.headerSize + 20 > bytes.length) {
    return { ...structure, durationInSeconds: null };
  }
  const payload = mvhd.offset + mvhd.headerSize;
  const version = bytes[payload];
  let timescale;
  let duration;
  if (version === 1 && payload + 32 <= bytes.length) {
    timescale = uint32be(bytes, payload + 20);
    duration = (BigInt(uint32be(bytes, payload + 24)) << 32n) | BigInt(uint32be(bytes, payload + 28));
  } else if (payload + 20 <= bytes.length) {
    timescale = uint32be(bytes, payload + 12);
    duration = BigInt(uint32be(bytes, payload + 16));
  }
  const durationInSeconds = timescale > 0 && duration !== undefined
    ? Number(duration) / timescale
    : null;
  const tracks = isoChildren(bytes, moov.headerSize, bytes.length)
    .filter((box) => box.type === 'trak')
    .map((trak) => {
      const tkhd = isoPath(bytes, trak, ['tkhd']);
      let trackWidth = null;
      let trackHeight = null;
      let trackId = null;
      if (tkhd) {
        const tkhdPayload = tkhd.offset + tkhd.headerSize;
        const version = bytes[tkhdPayload];
        trackId = uint32be(bytes, tkhdPayload + (version === 1 ? 20 : 12));
        const geomOffset = tkhdPayload + (version === 1 ? 88 : 76);
        if (geomOffset + 8 <= tkhd.offset + tkhd.size) {
          trackWidth = uint32be(bytes, geomOffset) >>> 16;
          trackHeight = uint32be(bytes, geomOffset + 4) >>> 16;
        }
      }
      const mdia = isoPath(bytes, trak, ['mdia']);
      const mdhd = mdia && isoPath(bytes, mdia, ['mdhd']);
      const hdlr = mdia && isoPath(bytes, mdia, ['hdlr']);
      if (!mdhd || !hdlr) return null;
      const mdhdPayload = mdhd.offset + mdhd.headerSize;
      const version = bytes[mdhdPayload];
      const trackTimescale = uint32be(bytes, mdhdPayload + (version === 1 ? 20 : 12));
      const trackDuration = version === 1
        ? (BigInt(uint32be(bytes, mdhdPayload + 24)) << 32n) | BigInt(uint32be(bytes, mdhdPayload + 28))
        : BigInt(uint32be(bytes, mdhdPayload + 16));
      const handlerPayload = hdlr.offset + hdlr.headerSize;
      const handler = ascii(bytes, handlerPayload + 8, 4);
      const table = (name) => isoPath(bytes, trak, ['mdia', 'minf', 'stbl', name]);
      const stts = table('stts');
      const stsz = table('stsz');
      const stco = table('stco') ?? table('co64');
      const stss = table('stss');
      const ctts = table('ctts');
      const readEntries = (box, width, valueOffset) => {
        if (!box) return [];
        const payload = box.offset + box.headerSize;
        const count = uint32be(bytes, payload + valueOffset);
        if (!Number.isSafeInteger(count) || count > 1_000_000) return [];
        const values = [];
        let cursor = payload + valueOffset + 4;
        for (let index = 0; index < count && cursor + width <= box.offset + box.size; index += 1) {
          values.push(width === 8
            ? Number((BigInt(uint32be(bytes, cursor)) << 32n) | BigInt(uint32be(bytes, cursor + 4)))
            : uint32be(bytes, cursor));
          cursor += width;
        }
        return values;
      };
      const sampleSizePayload = stsz && stsz.offset + stsz.headerSize;
      const uniformSampleSize = sampleSizePayload ? uint32be(bytes, sampleSizePayload + 4) : 0;
      const sampleCount = sampleSizePayload ? uint32be(bytes, sampleSizePayload + 8) : 0;
      const stscBox = table('stsc');
      const stscEntries = stscBox ? (() => {
        const payload = stscBox.offset + stscBox.headerSize;
        const count = uint32be(bytes, payload + 4);
        const entries = [];
        for (let index = 0; index < count && index < 1_000_000; index += 1) {
          const cursor = payload + 8 + index * 12;
          if (cursor + 12 > stscBox.offset + stscBox.size) break;
          entries.push({
            firstChunk: uint32be(bytes, cursor),
            samplesPerChunk: uint32be(bytes, cursor + 4),
            sampleDescriptionIndex: uint32be(bytes, cursor + 8),
          });
        }
        return entries;
      })() : [];
      const chunkOffsets = readEntries(stco, stco?.type === 'co64' ? 8 : 4, 4);
      const sampleSizes = uniformSampleSize ? [] : readEntries(stsz, 4, 8);
      const keyframes = readEntries(stss, 4, 4);
      const keyframeSet = new Set(keyframes);
      const codecConfig = findCodecConfig(bytes, trak);
      const audioConfig = codecConfig?.type === 'esds'
        ? parseAacAudioSpecificConfig(codecConfig.data)
        : null;
      const sampleRanges = [];
      let sampleIndex = 0;
      for (let chunkIndex = 0; chunkIndex < chunkOffsets.length && sampleIndex < Math.min(sampleCount, 1_000_000); chunkIndex += 1) {
        const chunkNumber = chunkIndex + 1;
        let entry = stscEntries[0];
        for (const candidate of stscEntries) {
          if (candidate.firstChunk <= chunkNumber) entry = candidate;
          else break;
        }
        if (!entry) continue;
        let cursor = chunkOffsets[chunkIndex];
        for (let inChunk = 0; inChunk < entry.samplesPerChunk && sampleIndex < sampleCount && sampleIndex < 1_000_000; inChunk += 1) {
          const size = uniformSampleSize || sampleSizes[sampleIndex] || 0;
          sampleRanges.push({
            sampleIndex, offset: cursor, size,
            sampleDescriptionIndex: entry.sampleDescriptionIndex,
            keyframe: !stss || keyframeSet.has(sampleIndex + 1),
          });
          cursor += size;
          sampleIndex += 1;
        }
      }
      const timeToSample = stts ? (() => {
        const payload = stts.offset + stts.headerSize;
        const count = uint32be(bytes, payload + 4);
        const entries = [];
        for (let index = 0; index < count && index < 1_000_000; index += 1) {
          const cursor = payload + 8 + index * 8;
          if (cursor + 8 > stts.offset + stts.size) break;
          entries.push({ count: uint32be(bytes, cursor), delta: uint32be(bytes, cursor + 4) });
        }
        return entries;
      })() : [];
      const compositionOffsets = ctts ? (() => {
        const payload = ctts.offset + ctts.headerSize;
        const version = bytes[payload];
        const count = uint32be(bytes, payload + 4);
        const entries = [];
        for (let index = 0; index < count && index < 1_000_000; index += 1) {
          const cursor = payload + 8 + index * 8;
          if (cursor + 8 > ctts.offset + ctts.size) break;
          let offset = uint32be(bytes, cursor + 4);
          if (version === 1 && offset & 0x80000000) offset -= 0x100000000;
          entries.push({ count: uint32be(bytes, cursor), offset });
        }
        return entries;
      })() : [];
      const sampleTimestamps = [];
      let decodeTime = 0;
      let timedSamples = 0;
      for (const entry of timeToSample) {
        for (let index = 0; index < entry.count && timedSamples < sampleRanges.length; index += 1) {
          sampleTimestamps.push({
            sampleIndex: timedSamples,
            timestamp: trackTimescale ? decodeTime / trackTimescale : 0,
            duration: trackTimescale ? entry.delta / trackTimescale : 0,
          });
          decodeTime += entry.delta;
          timedSamples += 1;
        }
        if (timedSamples >= sampleRanges.length) break;
      }
      const compositionTimestamps = [];
      let compositionSample = 0;
      for (const entry of compositionOffsets) {
        for (let index = 0; index < entry.count && compositionSample < sampleRanges.length; index += 1) {
          compositionTimestamps.push({ sampleIndex: compositionSample, offset: entry.offset });
          compositionSample += 1;
        }
        if (compositionSample >= sampleRanges.length) break;
      }
      const compositionMap = new Map(compositionTimestamps.map((sample) => [sample.sampleIndex, sample.offset]));
      const timedSampleMap = new Map(sampleTimestamps.map((sample) => [sample.sampleIndex, sample]));
      const firstDelta = sampleTimestamps.length > 1
        ? sampleTimestamps[1].timestamp - sampleTimestamps[0].timestamp
        : 0;
      const timedRanges = sampleRanges.map((sample) => ({
        ...sample,
        timestamp: timedSampleMap.get(sample.sampleIndex)?.timestamp ?? null,
        duration: timedSampleMap.get(sample.sampleIndex)?.duration ?? null,
        compositionOffset: compositionMap.get(sample.sampleIndex) ?? 0,
        presentationTimestamp: timedSampleMap.has(sample.sampleIndex)
          ? timedSampleMap.get(sample.sampleIndex).timestamp + (trackTimescale ? (compositionMap.get(sample.sampleIndex) ?? 0) / trackTimescale : 0)
          : null,
      }));
      return {
        id: trackId,
        width: trackWidth,
        height: trackHeight,
        type: handler === 'vide' ? 'video' : handler === 'soun' ? 'audio' : 'unknown',
        handler,
        timescale: trackTimescale,
        durationInSeconds: trackTimescale ? Number(trackDuration) / trackTimescale : null,
        fps: firstDelta > 0 ? 1 / firstDelta : null,
        sampleRate: audioConfig?.sampleRate ?? null,
        numberOfChannels: audioConfig?.channels ?? null,
        sampleTables: {
          timeToSample,
          compositionOffsets,
          sampleToChunk: stscEntries,
          sampleCount: Number.isSafeInteger(sampleCount) ? sampleCount : 0,
          uniformSampleSize: uniformSampleSize || null,
          sampleSizes,
          chunkOffsets,
          keyframes,
          sampleRanges: timedRanges,
          sampleTimestamps,
          compositionTimestamps,
        },
        codecConfig,
      };
    }).filter(Boolean);
  return { ...structure, durationInSeconds: Number.isFinite(durationInSeconds) ? durationInSeconds : null, tracks };
}

// Fetch exactly one ISO-BMFF sample. The returned payload is decoder-neutral;
// callers can feed it to WebCodecs after applying the codec-specific
// avcC/hvcC/av1C conversion required by the selected track.
export async function readIsoBmffSample(source, trackIndex, sampleIndex, options = {}) {
  if (!Number.isInteger(trackIndex) || trackIndex < 0 || !Number.isInteger(sampleIndex) || sampleIndex < 0) {
    throw new RangeError('readIsoBmffSample expects non-negative integer indexes');
  }
  const parsed = options.parsed ?? await parseIsoBmffMovieHeader(source, options);
  const track = parsed?.tracks?.[trackIndex];
  const sample = track?.sampleTables?.sampleRanges?.[sampleIndex];
  if (!sample) throw new RangeError(`sample ${sampleIndex} is not available on track ${trackIndex}`);
  if (!Number.isSafeInteger(sample.offset) || !Number.isSafeInteger(sample.size) || sample.size <= 0) {
    throw new Error(`sample ${sampleIndex} has an invalid byte range`);
  }
  const data = await readMediaRange(source, sample.offset, sample.offset + sample.size, options);
  return { ...sample, data, trackIndex };
}

export async function readIsoBmffSamples(source, trackIndex, sampleIndexes, options = {}) {
  if (!Array.isArray(sampleIndexes)) throw new TypeError('readIsoBmffSamples expects an array of sample indexes');
  const concurrency = Number.isInteger(options.concurrency) && options.concurrency > 0 ? options.concurrency : 4;
  const parsed = options.parsed ?? await parseIsoBmffMovieHeader(source, options);
  const output = new Array(sampleIndexes.length);
  let cursor = 0;
  const worker = async () => {
    while (cursor < sampleIndexes.length) {
      const index = cursor++;
      output[index] = await readIsoBmffSample(source, trackIndex, sampleIndexes[index], { ...options, parsed });
    }
  };
  await Promise.all(Array.from({ length: Math.min(concurrency, sampleIndexes.length) }, worker));
  return output;
}

// Convert a fetched sample to the WebCodecs encoded-chunk contract. Keeping
// this small adapter separate from the parser lets native hosts consume the
// same sample metadata without depending on browser globals.
export function createIsoBmffEncodedChunk(sample, type = 'video') {
  if (!sample?.data || !(sample.data instanceof Uint8Array)) {
    throw new TypeError('createIsoBmffEncodedChunk expects a sample with Uint8Array data');
  }
  if (typeof EncodedVideoChunk === 'undefined' && type === 'video') {
    throw new Error('EncodedVideoChunk is not available in this runtime');
  }
  if (typeof EncodedAudioChunk === 'undefined' && type === 'audio') {
    throw new Error('EncodedAudioChunk is not available in this runtime');
  }
  const timestamp = Math.round((sample.presentationTimestamp ?? sample.timestamp ?? 0) * 1_000_000);
  const duration = sample.duration == null ? undefined : Math.max(0, Math.round(sample.duration * 1_000_000));
  if (type === 'audio') return new EncodedAudioChunk({
    type: sample.keyframe === false ? 'delta' : 'key', timestamp, duration, data: sample.data,
  });
  return new EncodedVideoChunk({
    type: sample.keyframe ? 'key' : 'delta', timestamp, duration, data: sample.data,
  });
}

export function makeIsoBmffWebCodecsConfig(track) {
  const config = track?.codecConfig;
  if (!config?.data || !(config.data instanceof Uint8Array)) {
    throw new TypeError('makeIsoBmffWebCodecsConfig expects a parsed track codecConfig');
  }
  if (config.type === 'avcC' && config.data.length >= 4) {
    const hex = (value) => value.toString(16).padStart(2, '0');
    return {
      codec: `avc1.${hex(config.data[1])}${hex(config.data[2])}${hex(config.data[3])}`,
      description: config.data,
      format: 'avc',
    };
  }
  if (config.type === 'hvcC') return { codec: 'hvc1.1.6.L93.B0', description: config.data, format: 'hevc' };
  if (config.type === 'av1C') return { codec: 'av01.0.08M.08', description: config.data, format: 'av1' };
  if (config.type === 'esds') {
    for (let offset = 4; offset < config.data.length - 2; offset += 1) {
      if (config.data[offset] !== 0x05) continue;
      let length = 0; let cursor = offset + 1; let byte;
      do {
        if (cursor >= config.data.length) break;
        byte = config.data[cursor++];
        length = (length << 7) | (byte & 0x7f);
      } while (byte & 0x80);
      if (cursor + length <= config.data.length) {
        return { codec: 'mp4a.40.2', description: config.data.slice(cursor, cursor + length), format: 'aac' };
      }
    }
    throw new Error('AAC esds does not contain an AudioSpecificConfig descriptor');
  }
  throw new Error(`unsupported ISO-BMFF codec configuration: ${config.type}`);
}

export function avccToAnnexB(data, lengthSize = 4) {
  if (!(data instanceof Uint8Array) || ![1, 2, 4].includes(lengthSize)) {
    throw new TypeError('avccToAnnexB expects Uint8Array data and a length size of 1, 2, or 4');
  }
  const output = [];
  let offset = 0;
  while (offset + lengthSize <= data.length) {
    let length = 0;
    for (let index = 0; index < lengthSize; index += 1) length = length * 256 + data[offset + index];
    offset += lengthSize;
    if (length <= 0 || offset + length > data.length) throw new RangeError('invalid AVCC NAL unit length');
    output.push(0, 0, 0, 1, ...data.subarray(offset, offset + length));
    offset += length;
  }
  if (offset !== data.length) throw new RangeError('truncated AVCC sample');
  return new Uint8Array(output);
}

// Decode a bounded sequence of ISO-BMFF samples with Chromium WebCodecs.
// `codecConfig` is caller-supplied because avcC/hvcC normalization and codec
// string selection depend on the sample entry; returned VideoFrames belong to
// the caller and must be closed when no longer needed.
export async function decodeIsoBmffVideo(source, {
  trackIndex = 0, startSample = 0, endSample = Infinity, maxSamples = 256, codec, description, options = {},
  onFrame,
} = {}) {
  if (typeof VideoDecoder === 'undefined') throw new Error('VideoDecoder is not available in this runtime');
  if (typeof codec !== 'string' || !codec) throw new TypeError('decodeIsoBmffVideo requires a codec string');
  const parsed = await parseIsoBmffMovieHeader(source, options);
  const track = parsed?.tracks?.[trackIndex];
  const ranges = track?.sampleTables?.sampleRanges ?? [];
  if (!Number.isInteger(maxSamples) || maxSamples <= 0) throw new RangeError('maxSamples must be a positive integer');
  const samples = ranges.filter(({ sampleIndex }) => sampleIndex >= startSample && sampleIndex < endSample).slice(0, maxSamples);
  if (!samples.length) throw new RangeError('decodeIsoBmffVideo found no samples in the requested range');
  if (onFrame !== undefined && typeof onFrame !== 'function') throw new TypeError('onFrame must be a function');
  const frames = onFrame ? null : [];
  let failure;
  const decoder = new VideoDecoder({
    output: (frame) => { if (onFrame) onFrame(frame); else frames.push(frame); },
    error: (error) => { failure = error; },
  });
  const decoderConfig = { codec, ...(description ? { description } : {}) };
  if (typeof VideoDecoder.isConfigSupported === 'function') {
    const support = await VideoDecoder.isConfigSupported(decoderConfig);
    if (!support.supported) {
      decoder.close();
      throw new Error(`WebCodecs does not support video codec: ${codec}`);
    }
  }
  decoder.configure(decoderConfig);
  try {
    const payloads = await readIsoBmffSamples(source, trackIndex, samples.map(({ sampleIndex }) => sampleIndex), {
      ...options, parsed, concurrency: options.concurrency ?? 4,
    });
    for (const sample of payloads) {
      decoder.decode(createIsoBmffEncodedChunk(sample));
    }
    await decoder.flush();
    if (failure) throw failure;
    return frames;
  } catch (error) {
    for (const frame of frames ?? []) frame.close();
    throw error;
  } finally {
    if (decoder.state !== 'closed') decoder.close();
  }
}

// Convert one WebCodecs VideoFrame to the native transport contract. The
// returned bytes are tightly packed, top-to-bottom RGBA8 pixels; callers own
// the Uint8Array and must still close the input VideoFrame.
export async function videoFrameToRgba(frame) {
  if (!frame || typeof frame.copyTo !== 'function') {
    throw new TypeError('videoFrameToRgba expects a VideoFrame');
  }
  const width = frame.displayWidth ?? frame.codedWidth;
  const height = frame.displayHeight ?? frame.codedHeight;
  if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0) {
    throw new RangeError('VideoFrame has invalid display dimensions');
  }
  const rgba = new Uint8Array(width * height * 4);
  await frame.copyTo(rgba, {
    format: 'RGBA',
    layout: [{ offset: 0, stride: width * 4 }],
  });
  return { width, height, rgba };
}

export async function decodeIsoBmffAudio(source, {
  trackIndex = 0, startSample = 0, endSample = Infinity, maxSamples = 256, codec, description,
  numberOfChannels, sampleRate, options = {}, onAudioData,
} = {}) {
  if (typeof AudioDecoder === 'undefined') throw new Error('AudioDecoder is not available in this runtime');
  if (typeof codec !== 'string' || !codec) throw new TypeError('decodeIsoBmffAudio requires a codec string');
  const parsed = await parseIsoBmffMovieHeader(source, options);
  const track = parsed?.tracks?.[trackIndex];
  const ranges = track?.sampleTables?.sampleRanges ?? [];
  if (!Number.isInteger(maxSamples) || maxSamples <= 0) throw new RangeError('maxSamples must be a positive integer');
  const samples = ranges.filter(({ sampleIndex }) => sampleIndex >= startSample && sampleIndex < endSample).slice(0, maxSamples);
  if (!samples.length) throw new RangeError('decodeIsoBmffAudio found no samples in the requested range');
  if (onAudioData !== undefined && typeof onAudioData !== 'function') throw new TypeError('onAudioData must be a function');
  const chunks = onAudioData ? null : [];
  let failure;
  const decoder = new AudioDecoder({
    output: (audio) => { if (onAudioData) onAudioData(audio); else chunks.push(audio); },
    error: (error) => { failure = error; },
  });
  const decoderConfig = {
    codec,
    ...(description ? { description } : {}),
    ...(Number.isInteger(numberOfChannels) ? { numberOfChannels } : {}),
    ...(Number.isInteger(sampleRate) ? { sampleRate } : {}),
  };
  if (typeof AudioDecoder.isConfigSupported === 'function') {
    const support = await AudioDecoder.isConfigSupported(decoderConfig);
    if (!support.supported) {
      decoder.close();
      throw new Error(`WebCodecs does not support audio codec: ${codec}`);
    }
  }
  decoder.configure(decoderConfig);
  try {
    const payloads = await readIsoBmffSamples(source, trackIndex, samples.map(({ sampleIndex }) => sampleIndex), {
      ...options, parsed, concurrency: options.concurrency ?? 4,
    });
    for (const sample of payloads) {
      if (failure) throw failure;
      decoder.decode(createIsoBmffEncodedChunk(sample, 'audio'));
    }
    await decoder.flush();
    if (failure) throw failure;
    return onAudioData ? null : chunks;
  } catch (error) {
    for (const chunk of chunks ?? []) chunk.close();
    throw error;
  } finally {
    if (decoder.state !== 'closed') decoder.close();
  }
}

function uint32be(bytes, offset) {
  return ((bytes[offset] << 24) | (bytes[offset + 1] << 16) | (bytes[offset + 2] << 8) | bytes[offset + 3]) >>> 0;
}

// Browser equivalent of @remotion/media-utils/getAudioDurationInSeconds.
export async function getAudioDurationInSeconds(source) {
  if (typeof source !== 'string' || !source) throw new TypeError('getAudioDurationInSeconds expects a source URL');
  if (audioDurationCache.has(source)) return audioDurationCache.get(source);
  const duration = new Promise((resolve, reject) => {
    const audio = document.createElement('audio');
    const cleanup = () => { audio.removeAttribute('src'); audio.load(); };
    audio.preload = 'metadata';
    audio.onloadedmetadata = () => {
      if (!Number.isFinite(audio.duration)) {
        reject(new Error(`unable to determine audio duration: ${source}`));
        cleanup();
        return;
      }
      resolve(audio.duration);
      cleanup();
    };
    audio.onerror = () => { reject(new Error(`failed to load audio metadata: ${source}`)); cleanup(); };
    audio.src = source;
  }).catch((error) => {
    audioDurationCache.delete(source);
    throw error;
  });
  audioDurationCache.set(source, duration);
  return duration;
}

export const getAudioDuration = getAudioDurationInSeconds;

// Browser counterpart of @remotion/media-utils/getAudioData. Decode once per
// source and expose channel-major PCM data so audio visualizers can share the
// same frame-driven contract as native compositions.
export async function getAudioData(source, { sampleRate = 48_000, requestInit } = {}) {
  if (typeof source !== 'string' || !source) throw new TypeError('getAudioData expects a source URL');
  if (!Number.isFinite(sampleRate) || sampleRate <= 0) throw new TypeError('sampleRate must be positive');
  if (audioDataCache.has(source)) return audioDataCache.get(source);
  const task = (async () => {
    const AudioContext = window.AudioContext || window.webkitAudioContext;
    if (!AudioContext) throw new Error('Web Audio API is unavailable');
    const response = await fetch(source, requestInit);
    if (!response.ok) throw new Error(`failed to load audio data: ${source}`);
    const context = new AudioContext({ sampleRate });
    try {
      const buffer = await context.decodeAudioData(await response.arrayBuffer());
      const channelWaveforms = Array.from({ length: buffer.numberOfChannels }, (_, channel) =>
        buffer.getChannelData(channel));
      return {
        // `channelWaveforms` is Remotion's v4 field; `channelData` is kept as
        // the Web Audio-friendly alias used by browser visualizer libraries.
        channelWaveforms,
        channelData: channelWaveforms,
        sampleRate: buffer.sampleRate,
        durationInSeconds: buffer.duration,
        numberOfChannels: buffer.numberOfChannels,
        resultId: source,
        isRemote: /^https?:\/\//i.test(source),
      };
    } finally {
      await context.close();
    }
  })().catch((error) => {
    audioDataCache.delete(source);
    throw error;
  });
  audioDataCache.set(source, task);
  return task;
}

// Hook-shaped alias for browser compositions. Consumers can await the same
// promise from a frame render and use delayRender around it when necessary.
export const useAudioData = getAudioData;

// Remotion-compatible AudioBuffer -> float32 WAV data URL. Keeping this in
// the browser host lets generated compositions feed synthesized audio back
// into Html5Audio or an export request without a server round-trip.
export function audioBufferToDataUrl(buffer) {
  if (!buffer || !Number.isInteger(buffer.numberOfChannels) || buffer.numberOfChannels < 1) {
    throw new TypeError('audioBufferToDataUrl expects an AudioBuffer');
  }
  const channels = Array.from({ length: buffer.numberOfChannels }, (_, channel) => buffer.getChannelData(channel));
  const frames = channels[0].length;
  const interleaved = new Float32Array(frames * buffer.numberOfChannels);
  for (let frame = 0; frame < frames; frame += 1) {
    for (let channel = 0; channel < channels.length; channel += 1) {
      interleaved[frame * channels.length + channel] = channels[channel][frame] ?? 0;
    }
  }
  const bytesPerSample = 4;
  const blockAlign = channels.length * bytesPerSample;
  const output = new ArrayBuffer(44 + interleaved.length * bytesPerSample);
  const view = new DataView(output);
  const writeString = (offset, value) => [...value].forEach((character, index) => view.setUint8(offset + index, character.charCodeAt(0)));
  writeString(0, 'RIFF'); view.setUint32(4, 36 + interleaved.length * bytesPerSample, true);
  writeString(8, 'WAVE'); writeString(12, 'fmt '); view.setUint32(16, 16, true);
  view.setUint16(20, 3, true); view.setUint16(22, channels.length, true);
  view.setUint32(24, buffer.sampleRate, true); view.setUint32(28, buffer.sampleRate * blockAlign, true);
  view.setUint16(32, blockAlign, true); view.setUint16(34, 32, true); writeString(36, 'data');
  view.setUint32(40, interleaved.length * bytesPerSample, true);
  for (let index = 0; index < interleaved.length; index += 1) view.setFloat32(44 + index * 4, interleaved[index], true);
  const bytes = new Uint8Array(output);
  let binary = '';
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, Math.min(offset + 0x8000, bytes.length)));
  }
  return `data:audio/wav;base64,${window.btoa(binary)}`;
}

export function prefetch(source, {
  method = 'blob-url', credentials, contentType, onProgress,
} = {}) {
  if (typeof source !== 'string' || !source) throw new TypeError('prefetch expects a source URL');
  const hashIndex = source.indexOf('#');
  const base = hashIndex < 0 ? source : source.slice(0, hashIndex);
  const controller = new AbortController();
  let released = false;
  let objectUrl = null;
  const done = fetch(base, { credentials, signal: controller.signal }).then(async (response) => {
    if (!response.ok) throw new Error(`prefetch failed: ${response.status} ${response.statusText}`);
    if (!response.body) throw new Error('prefetch response has no body');
    const reader = response.body.getReader();
    const chunks = [];
    let received = 0;
    const total = Number(response.headers.get('content-length')) || null;
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
      received += value.byteLength;
      onProgress?.({ loadedBytes: received, totalBytes: total });
    }
    const blob = new Blob(chunks, { type: response.headers.get('content-type') || undefined });
    if (released) return base;
    if (method === 'base64') {
      const bytes = new Uint8Array(await blob.arrayBuffer());
      let binary = '';
      for (let offset = 0; offset < bytes.length; offset += 0x8000) {
        binary += String.fromCharCode(...bytes.subarray(offset, Math.min(offset + 0x8000, bytes.length)));
      }
      const type = contentType || blob.type || 'application/octet-stream';
      return `data:${type};base64,${window.btoa(binary)}`;
    }
    objectUrl = URL.createObjectURL(contentType ? new Blob([blob], { type: contentType }) : blob);
    return objectUrl;
  });
  done.then((resolved) => {
    if (!released) preloadedSources.set(base, resolved);
  }).catch(() => undefined);
  return {
    free() {
      released = true;
      controller.abort();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      preloadedSources.delete(base);
    },
    waitUntilDone: () => done,
  };
}

export function usePreload(source) {
  if (typeof source !== 'string') return source;
  const hashIndex = source.indexOf('#');
  const base = hashIndex < 0 ? source : source.slice(0, hashIndex);
  const suffix = hashIndex < 0 ? '' : source.slice(hashIndex);
  const loaded = preloadedSources.get(base);
  return loaded ? `${loaded}${suffix}` : source;
}

// Platform-neutral counterpart of Remotion's useWindowedAudioData. The host
// does not require React: callers receive the window centered on the requested
// frame plus its timeline offset, while getAudioData() keeps decoding cached.
export async function getWindowedAudioData(source, {
  frame, fps, windowInSeconds, channelIndex = 0,
} = {}) {
  if (!Number.isFinite(frame) || !Number.isFinite(fps) || fps <= 0) {
    throw new TypeError('getWindowedAudioData requires a finite frame and positive fps');
  }
  if (!Number.isFinite(windowInSeconds) || windowInSeconds <= 0) {
    throw new TypeError('windowInSeconds must be positive');
  }
  const audioData = await getAudioData(source);
  if (!Number.isInteger(channelIndex) || channelIndex < 0 || channelIndex >= audioData.numberOfChannels) {
    throw new RangeError(`Invalid channel index ${channelIndex} for ${audioData.numberOfChannels} channels`);
  }
  const currentTime = frame / fps;
  const windowIndex = Math.floor(currentTime / windowInSeconds);
  const startTime = windowIndex * windowInSeconds;
  const startSample = Math.max(0, Math.floor(startTime * audioData.sampleRate));
  const endSample = Math.min(
    audioData.channelWaveforms[channelIndex].length,
    Math.ceil((startTime + windowInSeconds) * audioData.sampleRate),
  );
  const waveform = audioData.channelWaveforms[channelIndex].slice(startSample, endSample);
  return {
    audioData: {
      ...audioData,
      channelWaveforms: [waveform],
      channelData: [waveform],
      numberOfChannels: 1,
      durationInSeconds: waveform.length / audioData.sampleRate,
      resultId: `${audioData.resultId}:window:${channelIndex}:${windowIndex}`,
    },
    dataOffsetInSeconds: startSample / audioData.sampleRate,
  };
}

// In the framework-free host this hook-shaped name is an async adapter rather
// than a React hook; it preserves the vendor import name without requiring a
// React runtime in Three.js compositions.
export const useWindowedAudioData = getWindowedAudioData;

// Lightweight browser equivalent of getWaveformPortion(). It preserves the
// frame/time contract while reducing decoded PCM into visualization bars.
export function getWaveformPortion({
  audioData, startTimeInSeconds, durationInSeconds, numberOfSamples,
  channel = 0, dataOffsetInSeconds = 0, outputRange = 'zero-to-one', normalize = true,
}) {
  const channels = audioData?.channelWaveforms ?? audioData?.channelData;
  if (!channels?.length || numberOfSamples <= 0) return [];
  const waveform = channels[Math.min(channel, channels.length - 1)];
  const start = Math.floor((startTimeInSeconds - dataOffsetInSeconds) * audioData.sampleRate);
  const end = Math.floor((startTimeInSeconds - dataOffsetInSeconds + durationInSeconds) * audioData.sampleRate);
  const padded = new Float32Array(Math.max(0, end - start));
  for (let sample = Math.max(0, start); sample < Math.min(waveform.length, end); sample += 1) {
    padded[sample - start] = waveform[sample];
  }
  const blockSize = Math.floor(padded.length / numberOfSamples);
  if (blockSize === 0) return [];
  const values = Array.from({ length: numberOfSamples }, (_, index) => {
    let sum = 0;
    for (let sample = 0; sample < blockSize; sample += 1) {
      sum += Math.abs(padded[index * blockSize + sample]);
    }
    return sum / blockSize;
  });
  const scale = normalize ? Math.max(...values, 1e-9) : 1;
  return values.map((value, index) => ({
    index,
    amplitude: outputRange === 'minus-one-to-one'
      ? (value / scale) * (index % 2 === 0 ? -1 : 1)
      : value / scale,
  }));
}

export function visualizeAudioWaveform({
  audioData, frame, fps, windowInSeconds, numberOfSamples,
  channel = 0, dataOffsetInSeconds = 0, normalize = false,
}) {
  if (windowInSeconds * audioData.sampleRate < numberOfSamples) {
    throw new TypeError('windowInSeconds must provide at least one audio sample per bar');
  }
  return getWaveformPortion({
    audioData,
    startTimeInSeconds: frame / fps - windowInSeconds / 2,
    durationInSeconds: windowInSeconds,
    numberOfSamples,
    channel,
    dataOffsetInSeconds,
    outputRange: 'minus-one-to-one',
    normalize,
  }).map(({ amplitude }) => amplitude);
}

const visualizeAudioCache = new Map();

function fftMagnitudes(samples) {
  const size = samples.length;
  const real = new Float64Array(samples);
  const imaginary = new Float64Array(size);
  for (let i = 1, j = 0; i < size; i += 1) {
    let bit = size >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      const value = real[i]; real[i] = real[j]; real[j] = value;
    }
  }
  for (let length = 2; length <= size; length <<= 1) {
    const angle = -2 * Math.PI / length;
    for (let offset = 0; offset < size; offset += length) {
      for (let i = 0; i < length / 2; i += 1) {
        const phase = angle * i;
        const even = offset + i;
        const odd = even + length / 2;
        const cos = Math.cos(phase);
        const sin = Math.sin(phase);
        const oddReal = real[odd] * cos - imaginary[odd] * sin;
        const oddImaginary = real[odd] * sin + imaginary[odd] * cos;
        real[odd] = real[even] - oddReal;
        imaginary[odd] = imaginary[even] - oddImaginary;
        real[even] += oddReal;
        imaginary[even] += oddImaginary;
      }
    }
  }
  return Array.from({ length: size / 2 }, (_, index) =>
    Math.hypot(real[index], imaginary[index]));
}

function accurateFftMagnitudes(samples) {
  const size = samples.length;
  const complex = accurateFftComplex(samples);
  return Array.from({ length: size / 2 }, (_, index) =>
    Math.hypot(complex[index * 2], complex[index * 2 + 1]));
}

function accurateFftComplex(samples) {
  const size = samples.length;
  if (size === 1) return new Float64Array([samples[0], 0]);
  const evens = new Float64Array(size / 2);
  const odds = new Float64Array(size / 2);
  for (let index = 0; index < size / 2; index += 1) {
    evens[index] = samples[index * 2];
    odds[index] = samples[index * 2 + 1];
  }
  const even = accurateFftComplex(evens);
  const odd = accurateFftComplex(odds);
  const result = new Float64Array(size * 2);
  for (let index = 0; index < size; index += 2) {
    const angle = -Math.PI * index / size;
    const oddReal = odd[index] * Math.cos(angle) - odd[index + 1] * Math.sin(angle);
    const oddImaginary = odd[index] * Math.sin(angle) + odd[index + 1] * Math.cos(angle);
    result[index] = even[index / 2] + oddReal;
    result[index + 1] = even[index / 2 + 1] + oddImaginary;
    result[index + size] = even[index / 2] - oddReal;
    result[index + size + 1] = even[index / 2 + 1] - oddImaginary;
  }
  return result;
}

export function visualizeAudio({
  audioData, frame, fps, numberOfSamples, optimizeFor = 'accuracy',
  dataOffsetInSeconds = 0, smoothing = true,
}) {
  const size = numberOfSamples * 2;
  if (!Number.isInteger(numberOfSamples) || numberOfSamples <= 0 || (size & (size - 1)) !== 0) {
    throw new TypeError(`numberOfSamples must produce a power-of-two FFT size; got ${numberOfSamples}`);
  }
  if (!fps) throw new TypeError('fps is required');
  const waveform = (audioData?.channelWaveforms ?? audioData?.channelData)?.[0];
  if (!waveform || waveform.length < size) throw new TypeError(`Audio data is not big enough to provide ${size} bars.`);
  const start = Math.floor((frame / fps - dataOffsetInSeconds) * audioData.sampleRate);
  const actualStart = Math.max(0, start - size / 2);
  const cacheKey = `${audioData.resultId}:${frame}:${fps}:${numberOfSamples}:${optimizeFor}:${dataOffsetInSeconds}`;
  const compute = () => {
    const samples = new Float64Array(size);
    for (let i = 0; i < size; i += 1) {
      const value = waveform[actualStart + i] ?? 0;
      samples[i] = Math.max(-1, Math.min(1, value)) * 32767;
    }
    const magnitudes = optimizeFor === 'accuracy'
      ? accurateFftMagnitudes(samples)
      : fftMagnitudes(samples);
    let maxMagnitude = 0;
    for (const sample of waveform) maxMagnitude = Math.max(maxMagnitude, Math.abs(sample));
    const maxInt = maxMagnitude * 32767 || 1;
    return magnitudes.map((value) => Math.max(0, Math.min(1, value / (size / 2) / maxInt)));
  };
  const current = visualizeAudioCache.get(cacheKey) ?? compute();
  visualizeAudioCache.set(cacheKey, current);
  if (!smoothing) return current;
  const neighbours = [frame - 1, frame + 1].map((nearbyFrame) => visualizeAudio({
    audioData, frame: nearbyFrame, fps, numberOfSamples, optimizeFor,
    dataOffsetInSeconds, smoothing: false,
  }));
  return current.map((value, index) => (value + neighbours[0][index] + neighbours[1][index]) / 3);
}

// Remotion-compatible Catmull-Rom-style SVG path helper for browser scenes.
export function createSmoothSvgPath({ points = [] } = {}) {
  const line = (a, b) => ({
    length: Math.hypot(b.x - a.x, b.y - a.y),
    angle: Math.atan2(b.y - a.y, b.x - a.x),
  });
  const controlPoint = (current, previous, next, reverse) => {
    const previousPoint = previous || current;
    const nextPoint = next || current;
    const opposed = line(previousPoint, nextPoint);
    const angle = opposed.angle + (reverse ? Math.PI : 0);
    const length = opposed.length * 0.2;
    return { x: current.x + Math.cos(angle) * length, y: current.y + Math.sin(angle) * length };
  };
  return points.reduce((path, current, index, all) => {
    if (index === 0) return `M ${current.x},${current.y}`;
    const previous = all[index - 1];
    const cp1 = controlPoint(previous, all[index - 2], current, false);
    const cp2 = controlPoint(current, previous, all[index + 1], true);
    return `${path} C ${cp1.x},${cp1.y} ${cp2.x},${cp2.y} ${current.x},${current.y}`;
  }, '');
}

// Browser equivalent of Remotion's useVideoTexture for non-React Three.js
// compositions. The element and texture are cached by source so a frame
// callback can reuse GPU resources across the entire render.
export async function getVideoTexture(source, options = {}) {
  await ensureThree();
  if (typeof source !== 'string' || !source) throw new TypeError('getVideoTexture expects a source URL');
  const cached = videoTextureCache.get(source);
  if (cached) {
    if (typeof options.muted === 'boolean') cached.video.muted = options.muted;
    if (typeof options.loop === 'boolean') cached.video.loop = options.loop;
    if (Number.isFinite(Number(options.playbackRate)) && Number(options.playbackRate) > 0) {
      cached.video.playbackRate = Number(options.playbackRate);
    }
    await seekVideoTexture(cached, options);
    return cached.texture;
  }
  const video = document.createElement('video');
  video.preload = 'auto';
  video.muted = options.muted ?? true;
  video.loop = options.loop ?? false;
  video.playsInline = true;
  if (Number.isFinite(Number(options.playbackRate)) && Number(options.playbackRate) > 0) {
    video.playbackRate = Number(options.playbackRate);
  }
  video.src = source;
  const ready = video.readyState >= 2 ? Promise.resolve() : new Promise((resolve, reject) => {
    video.addEventListener('loadeddata', resolve, { once: true });
    video.addEventListener('error', () => reject(new Error(`failed to load video texture: ${source}`)), { once: true });
  });
  await ready;
  const texture = new THREE.VideoTexture(video);
  texture.colorSpace = THREE.SRGBColorSpace;
  const entry = {
    video,
    texture,
    frame: null,
    playbackRate: null,
    startFrom: null,
    seekPromise: null,
  };
  videoTextureCache.set(source, entry);
  await seekVideoTexture(entry, options);
  return texture;
}

// Naming-compatible entry point for adapters ported from @remotion/three.
export const useVideoTexture = getVideoTexture;

// Browser-compatible counterpart of @remotion/three's
// useOffthreadVideoTexture. The worker owns deterministic frame seeking, so
// this reuses the cached texture while injecting the current composition frame.
export async function getOffthreadVideoTexture(source, options = {}) {
  return getVideoTexture(source, {
    ...options,
    frame: options.frame ?? frame,
    fps: options.fps ?? videoConfig.fps,
  });
}

export const useOffthreadVideoTexture = getOffthreadVideoTexture;

async function seekVideoTexture(entry, options) {
  const frame = Number(options.frame);
  const fps = Number(options.fps ?? 30);
  const playbackRate = Number(options.playbackRate ?? 1);
  const startFrom = Number(options.startFrom ?? 0);
  if (!Number.isFinite(frame) || !Number.isFinite(fps) || fps <= 0) return;
  if (!Number.isFinite(playbackRate) || playbackRate <= 0) return;
  const normalizedStartFrom = Number.isFinite(startFrom) ? startFrom : 0;
  if (entry.frame === frame && entry.playbackRate === playbackRate && entry.startFrom === normalizedStartFrom) return;
  if (entry.seekPromise) await entry.seekPromise;
  if (entry.frame === frame && entry.playbackRate === playbackRate && entry.startFrom === normalizedStartFrom) return;
  // Match Remotion's getExpectedMediaFrameUncorrected(): the media starts at
  // startFrom, and playbackRate applies only to frames after that point.
  const time = Math.max(0, (frame <= normalizedStartFrom
    ? frame
    : normalizedStartFrom + (frame - normalizedStartFrom) * playbackRate) / fps);
  const { video } = entry;
  if (Math.abs(video.currentTime - time) <= 1e-4 && video.readyState >= 2) {
    entry.frame = frame;
    entry.playbackRate = playbackRate;
    entry.startFrom = normalizedStartFrom;
    return;
  }
  entry.seekPromise = new Promise((resolve, reject) => {
    let settled = false;
    const finish = (error) => {
      if (settled) return;
      settled = true;
      video.removeEventListener('seeked', onSeeked);
      video.removeEventListener('error', onError);
      if (video.requestVideoFrameCallback && callbackId !== null) video.cancelVideoFrameCallback?.(callbackId);
      if (error) reject(error); else resolve();
    };
    const onSeeked = () => {
      if (!video.requestVideoFrameCallback) finish();
    };
    const onError = () => finish(new Error(`failed to seek video texture at frame ${frame}`));
    let callbackId = null;
    video.addEventListener('seeked', onSeeked, { once: true });
    video.addEventListener('error', onError, { once: true });
    if (video.requestVideoFrameCallback) {
      callbackId = video.requestVideoFrameCallback(() => finish());
    }
    video.currentTime = time;
  }).then(() => {
    entry.frame = frame;
    entry.playbackRate = playbackRate;
    entry.startFrom = normalizedStartFrom;
  });
  try {
    await entry.seekPromise;
  } finally {
    entry.seekPromise = null;
  }
}

export function releaseVideoTexture(source) {
  const cached = videoTextureCache.get(source);
  if (!cached) return false;
  cached.texture.dispose();
  cached.video.pause();
  cached.video.removeAttribute('src');
  cached.video.load();
  videoTextureCache.delete(source);
  return true;
}

export function clearMediaCaches() {
  for (const { video, texture } of videoTextureCache.values()) {
    texture.dispose();
    video.pause();
    video.removeAttribute('src');
    video.load();
  }
  videoTextureCache.clear();
  preloadedAssets.clear();
  preloadedSources.clear();
  imageDimensionsCache.clear();
  videoMetadataCache.clear();
  audioDurationCache.clear();
  audioDataCache.clear();
  webmMetadataCache.clear();
  webmClusterCache.clear();
  webmClusterCacheBytes = 0;
  return true;
}

// Optional browser ecosystem adapter. The core worker stays independent from
// lottie-web while applications can reuse any Lottie-compatible renderer.
export function registerLottieAdapter(adapter) {
  if (!adapter || typeof adapter.render !== 'function') {
    throw new TypeError('registerLottieAdapter expects an object with render()');
  }
  lottieAdapter = adapter;
}

const defaultLottieAdapter = {
  async render(element, state) {
    const lottie = (await (lottieModulePromise ??= import('lottie-web'))).default;
    let instance = lottieInstances.get(element);
    if (!instance || instance.src !== state.src) {
      instance?.animation.destroy();
      const preloaded = preloadedAssets.get(state.src);
      const animationData = preloaded && isJsonSource(state.src)
        ? await preloaded
        : undefined;
      const animation = lottie.loadAnimation({
        container: element,
        renderer: 'svg',
        loop: false,
        autoplay: false,
        ...(animationData ? { animationData } : { path: state.src }),
      });
      instance = { animation, src: state.src, ready: new Promise((resolve) => {
        animation.addEventListener('DOMLoaded', resolve, { once: true });
      }) };
      lottieInstances.set(element, instance);
    }
    await instance.ready;
    const totalFrames = Math.max(1, instance.animation.totalFrames || 1);
    const rawFrame = state.time * state.fps * state.playbackRate;
    const frame = state.loopBehavior === 'Loop'
      ? ((rawFrame % totalFrames) + totalFrames) % totalFrames
      : Math.max(0, Math.min(totalFrames - 1, rawFrame));
    element.style.visibility = state.loopBehavior === 'Unmount' && rawFrame >= totalFrames
      ? 'hidden' : '';
    instance.animation.goToAndStop(frame, true);
  },
};

lottieAdapter = defaultLottieAdapter;

export function listCompositions() {
  return [...new Set(['three_preview', ...compositions.keys()])];
}

// Remotion-compatible read-only hooks for browser compositions. They are
// updated at the start of every explicit render request, so adapters ported
// from React Three Fiber can use the familiar API without depending on React.
export function useCurrentFrame() {
  return frame;
}

export function useVideoConfig() {
  return { ...videoConfig };
}

export function getTimeEventFrame(name) {
  if (typeof name !== 'string' || !name.trim()) {
    throw new TypeError('getTimeEventFrame expects a non-empty event name');
  }
  const event = activeTimeEvents.find((item) => item.id === name);
  if (!event) throw new Error(`missing time event "${name}"`);
  return event.frame;
}

export function waitUntil(name) {
  return getTimeEventFrame(name);
}

export function getTimeEventDuration(startName, endName) {
  const startFrame = getTimeEventFrame(startName);
  const endFrame = getTimeEventFrame(endName);
  if (endFrame < startFrame) {
    throw new Error(`time event "${endName}" precedes "${startName}"`);
  }
  return endFrame - startFrame;
}

// Small compatibility layer for Remotion's static-file helpers. The browser
// host owns URL resolution, so compositions stay portable between Vite,
// packaged Tauri assets, and a remote preview origin.
export function staticFile(path) {
  if (typeof path !== 'string' || !path.trim()) {
    throw new TypeError('staticFile expects a non-empty path');
  }
  return new URL(path.replace(/^\/+/, ''), document.baseURI).toString();
}

export function getStaticFiles() {
  return [...activeAssets];
}

// Studio-compatible watcher. Vite/Tauri integrations can dispatch the same
// event with `{files: [{name, lastModified}]}` when a static asset changes;
// headless export naturally remains a no-op because no watcher is attached.
export function watchStaticFile(fileName, callback) {
  if (typeof fileName !== 'string' || typeof callback !== 'function') {
    throw new TypeError('watchStaticFile expects a file name and callback');
  }
  const normalized = fileName.replace(/^\/+/, '');
  let previous;
  const listener = (event) => {
    const files = event.detail?.files;
    if (!Array.isArray(files)) return;
    const next = files.find((file) => file?.name === normalized);
    if (!next && previous) { clearMediaCaches(); callback(null); }
    if (next && (!previous || next.lastModified !== previous.lastModified)) {
      clearMediaCaches();
      callback(next);
    }
    previous = next;
  };
  window.addEventListener('remotion_staticFilesChanged', listener);
  return { cancel: () => window.removeEventListener('remotion_staticFilesChanged', listener) };
}

// Remotion-compatible access to the current composition input props.
export function getInputProps() {
  return typeof structuredClone === 'function'
    ? structuredClone(activeProps)
    : JSON.parse(JSON.stringify(activeProps));
}

export function getRemotionEnvironment() {
  const isRendering = window.__DIOXUSCUT_HEADLESS_RENDER__ === true;
  return { isRendering, isStudio: !isRendering, isPlayer: false };
}

export const useRemotionEnvironment = getRemotionEnvironment;

// Explicit frame input keeps this scene deterministic for future exports.
async function renderDefaultFrame({ composition, frame: nextFrame, fps, props, width, height }) {
  await ensureThree();
  frame = nextFrame;
  if (Number.isFinite(width) && Number.isFinite(height)) {
    renderer.setSize(width, height, false);
    camera.aspect = width / Math.max(height, 1);
    camera.updateProjectionMatrix();
  }
  cube.rotation.x = nextFrame / Math.max(fps, 1) * 0.36;
  cube.rotation.y = nextFrame / Math.max(fps, 1) * 0.54;
  if (typeof props.color === 'string') cube.material.color.set(props.color);
  renderer.render(scene, camera);
  document.querySelector('#frame').textContent = `frame ${nextFrame}`;
  document.querySelector('#protocol').textContent = `composition ${composition}`;
}

async function syncMediaElements({ frame: nextFrame, fps }) {
  const timelineTime = nextFrame / Math.max(fps, 1);
  const pendingSeeks = [];
  for (const media of document.querySelectorAll('video, audio')) {
    const start = Number(media.dataset.timelineStart ?? 0);
    const duration = media.dataset.duration === undefined
      ? undefined
      : Number(media.dataset.duration);
    const end = duration === undefined ? undefined : start + duration;
    const active = timelineTime >= start && (end === undefined || timelineTime < end);
    media.style.visibility = active ? '' : 'hidden';
    if (!active) continue;

    const explicitTime = media.dataset.time ?? media.dataset.remotionSeek;
    const time = explicitTime === undefined
      ? undefined
      : Number(explicitTime);
    if (Number.isFinite(time) && Math.abs(media.currentTime - time) > 1e-4) {
      media.currentTime = Math.max(0, time);
      pendingSeeks.push(new Promise((resolve) => {
        const done = () => { media.removeEventListener('seeked', done); resolve(); };
        media.addEventListener('seeked', done, { once: true });
        setTimeout(done, 1000);
      }));
    }
    const volume = Number(media.dataset.volume ?? media.dataset.remotionVolume);
    if (Number.isFinite(volume)) media.volume = Math.max(0, Math.min(1, volume));
    const rate = Number(media.dataset.playbackRate ?? media.dataset.remotionPlaybackRate);
    if (Number.isFinite(rate) && rate > 0) media.playbackRate = rate;
    media.loop = media.hasAttribute('loop');
    media.pause();
  }
  await Promise.all(pendingSeeks);
}

function isJsonSource(source = '') {
  return source.split(/[?#]/, 1)[0].toLowerCase().endsWith('.json');
}

async function syncLottieElements({ frame: nextFrame, fps }) {
  if (!lottieAdapter) return;
  const elements = document.querySelectorAll('[data-dioxuscut-lottie]');
  await Promise.all([...elements].map(async (element) => {
    await lottieAdapter.render(element, {
      src: element.dataset.dioxuscutLottie,
      frame: nextFrame,
      fps,
      time: Number(element.dataset.time ?? 0),
      playbackRate: Number(element.dataset.playbackRate ?? 1),
      loopBehavior: element.dataset.loop ?? 'Loop',
    });
    element.dataset.frame = String(nextFrame);
  }));
}

async function syncCanvasImages(nextFrame) {
  const elements = document.querySelectorAll('[data-dioxuscut-canvas-image]');
  await Promise.all([...elements].map(async (element) => {
    const source = element.dataset.src;
    if (!source) return;
    const retries = Math.max(0, Number(element.dataset.maxRetries ?? 2));
    let asset;
    let lastError;
    for (let attempt = 0; attempt <= retries; attempt += 1) {
      try {
        asset = await (preloadedAssets.get(source) ?? preloadAssets([source]).then(() => preloadedAssets.get(source)));
        lastError = undefined;
        break;
      } catch (error) {
        lastError = error;
        preloadedAssets.delete(source);
        if (attempt < retries) await new Promise((resolve) => setTimeout(resolve, 50 * 2 ** attempt));
      }
    }
    if (lastError) {
      if (element.dataset.pauseWhenLoading === 'true') element.style.visibility = 'hidden';
      throw lastError;
    }
    const drawable = await asset;
    const width = Number(element.getAttribute('width')) || element.clientWidth || drawable.videoWidth || drawable.naturalWidth || 1;
    const height = Number(element.getAttribute('height')) || element.clientHeight || drawable.videoHeight || drawable.naturalHeight || 1;
    if (element.width !== width) element.width = width;
    if (element.height !== height) element.height = height;
    const context = element.getContext('2d');
    if (!context) return;
    context.clearRect(0, 0, width, height);
    const sourceWidth = drawable.videoWidth || drawable.naturalWidth || drawable.width || width;
    const sourceHeight = drawable.videoHeight || drawable.naturalHeight || drawable.height || height;
    const fit = element.dataset.fit ?? 'cover';
    const scale = fit === 'fill'
      ? { x: width / sourceWidth, y: height / sourceHeight }
      : fit === 'contain' || fit === 'scale-down'
        ? { x: Math.min(width / sourceWidth, height / sourceHeight), y: Math.min(width / sourceWidth, height / sourceHeight) }
        : fit === 'none'
          ? { x: 1, y: 1 }
          : { x: Math.max(width / sourceWidth, height / sourceHeight), y: Math.max(width / sourceWidth, height / sourceHeight) };
    const drawWidth = sourceWidth * scale.x;
    const drawHeight = sourceHeight * scale.y;
    context.drawImage(drawable, (width - drawWidth) / 2, (height - drawHeight) / 2, drawWidth, drawHeight);
    element.dataset.frame = String(nextFrame);
  }));
}

export async function renderFrame({ composition = 'three_preview', frame: nextFrame, fps = 30, props: inputProps = {}, assets = [], timeline = [], time_events: inputTimeEvents = [], width, height, durationInFrames }) {
  const props = inputProps && typeof inputProps === 'object' ? inputProps : {};
  frame = nextFrame;
  activeAssets = Array.isArray(assets) ? [...assets] : [];
  activeProps = props;
  activeTimeEvents = Array.isArray(inputTimeEvents) ? inputTimeEvents.map((event) => ({
    id: String(event?.id ?? ''), frame: Number(event?.frame),
  })) : [];
  videoConfig = {
    ...videoConfig,
    fps,
    ...(Number.isFinite(width) ? { width } : {}),
    ...(Number.isFinite(height) ? { height } : {}),
    ...(Number.isFinite(durationInFrames) ? { durationInFrames } : {}),
  };
  // A cancelled gate belongs to the current frame only. Reset it before the
  // next request so a transient asset/render cancellation does not poison the
  // rest of the composition.
  renderGateError = null;
  await preloadAssets(assets);
  // Match Remotion's render-ready gate: a frame is not capturable until all
  // declared font faces have finished loading.
  await document.fonts.ready;
  if (timeline.length > 0) {
    const projectContext = {
      frame,
      activeAssets,
      activeProps,
      activeTimeEvents,
      videoConfig,
    };
    const activeClips = timeline.filter((clip) =>
      nextFrame >= clip.start && nextFrame < clip.start + clip.duration);
    const hasThreeLayers = activeClips.some((clip) =>
      clip.composition === 'three_preview' || threeCompositions.has(clip.composition));
    if (hasThreeLayers) {
      await ensureThree();
      if (Number.isFinite(width) && Number.isFinite(height)) {
        renderer.setSize(width, height, false);
      }
      // Start with a clean color buffer, then preserve color between layers.
      // Clearing depth before every clip gives each scene an independent depth
      // buffer while retaining normal alpha blending in track order.
      renderer.clear(true, true, true);
    }
    // Three.js resets the drawing buffer whenever setSize() is called, even
    // when the dimensions have not changed. Timeline compositions commonly
    // call setSize(width, height) from render(), so preserve the shared color
    // buffer for same-size requests while restoring the viewport as usual.
    const originalSetSize = hasThreeLayers ? renderer.setSize : undefined;
    if (hasThreeLayers) {
      renderer.setSize = (requestedWidth, requestedHeight, updateStyle = true) => {
        const currentSize = renderer.getSize(new THREE.Vector2());
        if (requestedWidth === currentSize.x && requestedHeight === currentSize.y) {
          if (updateStyle) {
            canvas.style.width = `${requestedWidth}px`;
            canvas.style.height = `${requestedHeight}px`;
          }
          renderer.setViewport(0, 0, requestedWidth, requestedHeight);
          return;
        }
        return originalSetSize.call(renderer, requestedWidth, requestedHeight, updateStyle);
      };
    }
    try {
      for (const clip of activeClips) {
        const clipFrame = nextFrame - clip.start;
        const clipProps = clip.props && typeof clip.props === 'object' ? clip.props : {};
        frame = clipFrame;
        activeProps = clipProps;
        activeTimeEvents = projectContext.activeTimeEvents
          .filter((event) => event.frame >= clip.start && event.frame <= clip.start + clip.duration)
          .map((event) => ({ ...event, frame: event.frame - clip.start }));
        videoConfig = {
          ...projectContext.videoConfig,
          fps,
          ...(Number.isFinite(width) ? { width } : {}),
          ...(Number.isFinite(height) ? { height } : {}),
          durationInFrames: clip.duration,
        };

        const render = compositions.get(clip.composition) ??
          (clip.composition === 'three_preview' ? renderDefaultFrame : undefined);
        if (!render) throw new Error(`unknown browser composition: ${clip.composition}`);
        const isThreeLayer = clip.composition === 'three_preview' ||
          threeCompositions.has(clip.composition);
        const previousAutoClear = isThreeLayer ? renderer.autoClear : undefined;
        const previousAutoClearColor = isThreeLayer ? renderer.autoClearColor : undefined;
        const previousAutoClearDepth = isThreeLayer ? renderer.autoClearDepth : undefined;
        if (isThreeLayer) {
          renderer.autoClear = false;
          renderer.autoClearColor = false;
          renderer.autoClearDepth = false;
          renderer.clearDepth();
        }
        try {
          await render({
            composition: clip.composition,
            frame: clipFrame,
            fps,
            props: clipProps,
            assets,
            width,
            height,
            durationInFrames: clip.duration,
            timeEvents: activeTimeEvents.map((event) => ({ ...event })),
          });
        } finally {
          if (isThreeLayer) {
            renderer.autoClear = previousAutoClear;
            renderer.autoClearColor = previousAutoClearColor;
            renderer.autoClearDepth = previousAutoClearDepth;
          }
        }
      }
    } finally {
      if (hasThreeLayers) renderer.setSize = originalSetSize;
      frame = projectContext.frame;
      activeAssets = projectContext.activeAssets;
      activeProps = projectContext.activeProps;
      activeTimeEvents = projectContext.activeTimeEvents;
      videoConfig = projectContext.videoConfig;
    }
    await syncMediaElements({ frame: nextFrame, fps });
    await syncLottieElements({ frame: nextFrame, fps });
    await syncCanvasImages(nextFrame);
    await waitForRenderGates();
    return;
  }
  const customRender = compositions.get(composition);
  if (customRender) {
    const result = await customRender({
      composition,
      frame: nextFrame,
      fps,
      props,
      assets,
      width,
      height,
      durationInFrames,
      timeEvents: activeTimeEvents.map((event) => ({ ...event })),
    });
    await syncMediaElements({ frame: nextFrame, fps });
    await syncLottieElements({ frame: nextFrame, fps });
    await syncCanvasImages(nextFrame);
    await waitForRenderGates();
    return result;
  }
  const result = await renderDefaultFrame({ composition, frame: nextFrame, fps, props, width, height });
  await syncMediaElements({ frame: nextFrame, fps });
  await syncLottieElements({ frame: nextFrame, fps });
  await syncCanvasImages(nextFrame);
  await waitForRenderGates();
  return result;
}
window.dioxuscut = {
  renderFrame,
  registerComposition,
  registerThreeComposition,
  unregisterThreeComposition,
  listCompositions,
  delayRender,
  continueRender,
  cancelRender,
  useDelayRender,
  useBufferState,
  usePixelDensity,
  registerLottieAdapter,
  getVideoTexture,
  useVideoTexture,
  getOffthreadVideoTexture,
  useOffthreadVideoTexture,
  getImageDimensions,
  getVideoMetadata,
  parseMedia,
  readWebmSamples,
  createWebmEncodedVideoChunk,
  createWebmEncodedAudioChunk,
  decodeWebmVideo,
  decodeWebmAudio,
  parseWavMetadata,
  probeIsoBmff,
  parseIsoBmffMovieHeader,
  readIsoBmffSample,
  readIsoBmffSamples,
  createIsoBmffEncodedChunk,
  makeIsoBmffWebCodecsConfig,
  avccToAnnexB,
  decodeIsoBmffVideo,
  videoFrameToRgba,
  decodeIsoBmffAudio,
  readMediaRange,
  getAudioDurationInSeconds,
  getAudioDuration,
  getAudioData,
  useAudioData,
  audioBufferToDataUrl,
  prefetch,
  usePreload,
  getWindowedAudioData,
  useWindowedAudioData,
  getWaveformPortion,
  visualizeAudioWaveform,
  visualizeAudio,
  createSmoothSvgPath,
  releaseVideoTexture,
  clearMediaCaches,
  useCurrentFrame,
  useVideoConfig,
  getTimeEvents: () => activeTimeEvents.map((event) => ({ ...event })),
  getTimeEventFrame,
  waitUntil,
  getTimeEventDuration,
  staticFile,
  getStaticFiles,
  watchStaticFile,
  getInputProps,
  getRemotionEnvironment,
  useRemotionEnvironment,
};

function resize() {
  if (!renderer || !camera || window.__DIOXUSCUT_HEADLESS_RENDER__ === true) return;
  const { width, height } = canvas.parentElement.getBoundingClientRect();
  renderer.setSize(width, height, false);
  camera.aspect = width / Math.max(height, 1);
  camera.updateProjectionMatrix();
}
window.addEventListener('resize', resize);
resize();

let frame = 0;
let videoConfig = { fps: 30, width: 1280, height: 720, durationInFrames: 150 };
let activeAssets = [];
let activeProps = {};
let activeTimeEvents = [];
let playing = true;
let currentJobId = null;
let playbackStartedAt = performance.now();
let lastPlaybackFrame = -1;
let project = {
  version: 1,
  composition: 'three_preview',
  settings: { width: 1280, height: 720, fps: 30, duration: 150, backend: 'browser' },
  props: { color: '#6c63ff' }, assets: [], tracks: [],
  events: [], voice_over_asset_id: null,
};
const timelineCanvas = document.querySelector('#timeline-canvas');
const timelineScroll = document.querySelector('#timeline-scroll');
const timelineLabelWidth = 138;
const timelineZoomStops = [2, 3, 4, 6, 8, 12, 16];
let timelineScale = 4;
let selectedTrackId = null;
let selectedClipId = null;
let selectedTimeEventId = null;

function makeProjectId(prefix) {
  const suffix = globalThis.crypto?.randomUUID?.() ??
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `${prefix}-${suffix}`;
}

function projectTracks() {
  if (!Array.isArray(project.tracks)) project.tracks = [];
  return project.tracks;
}

function selectedClipEntry() {
  for (const track of projectTracks()) {
    if (track.id !== selectedTrackId) continue;
    const clip = track.clips?.find((item) => item.id === selectedClipId);
    if (clip) return { track, clip };
  }
  return null;
}

function updateClipInspector() {
  const inspector = document.querySelector('#clip-inspector');
  const entry = selectedClipEntry();
  document.querySelector('#duplicate-clip').disabled = !entry;
  document.querySelector('#remove-clip').disabled = !entry;
  if (!entry) {
    inspector.hidden = true;
    return;
  }

  const { track, clip } = entry;
  inspector.hidden = false;
  document.querySelector('#selected-clip-title').textContent = `${track.id} · ${clip.composition}`;
  document.querySelector('#clip-composition').value = clip.composition;
  document.querySelector('#clip-start').value = clip.start;
  document.querySelector('#clip-start').max = Math.max(0, project.settings.duration - clip.duration);
  document.querySelector('#clip-duration').value = clip.duration;
  document.querySelector('#clip-duration').max = Math.max(1, project.settings.duration - clip.start);
}

function selectedTimeEvent() {
  return (Array.isArray(project.events) ? project.events : [])
    .find((event) => event.id === selectedTimeEventId) ?? null;
}

function updateTimeEventInspector() {
  const inspector = document.querySelector('#time-event-inspector');
  const event = selectedTimeEvent();
  inspector.hidden = !event;
  if (!event) return;
  document.querySelector('#selected-time-event-title').textContent = `Event · ${event.id}`;
  document.querySelector('#time-event-name').value = event.id;
  document.querySelector('#time-event-frame').value = event.frame;
  document.querySelector('#time-event-frame').max = Math.max(0, project.settings.duration - 1);
}

function voiceOverAsset() {
  return project.assets?.find((asset) =>
    asset.id === project.voice_over_asset_id && String(asset.kind).toLowerCase() === 'audio') ?? null;
}

function voiceOverSource(path) {
  if (/^(https?:|data:|asset:)/i.test(path)) return path;
  const localPath = path.replace(/^file:\/\//i, '');
  return convertFileSrc(localPath);
}

function updateVoiceOverControls() {
  const select = document.querySelector('#voice-over-asset');
  const audio = document.querySelector('#voice-over-player');
  const assets = (Array.isArray(project.assets) ? project.assets : [])
    .filter((asset) => String(asset.kind).toLowerCase() === 'audio');
  select.replaceChildren(new Option('No voice-over', ''));
  for (const asset of assets) {
    select.add(new Option(asset.id, asset.id));
  }
  select.value = project.voice_over_asset_id ?? '';
  const asset = voiceOverAsset();
  if (!asset) {
    audio.pause();
    audio.removeAttribute('src');
    delete audio.dataset.pendingSeek;
    audio.load();
    audio.hidden = true;
    delete audio.dataset.assetId;
    return;
  }
  const source = voiceOverSource(asset.path);
  if (audio.dataset.assetId !== asset.id || audio.dataset.assetPath !== asset.path) {
    audio.pause();
    audio.src = source;
    audio.dataset.assetId = asset.id;
    audio.dataset.assetPath = asset.path;
    audio.load();
    seekVoiceOver(frame);
  }
  audio.hidden = false;
}

function seekVoiceOver(frameNumber) {
  const audio = document.querySelector('#voice-over-player');
  if (!audio.src) return;
  const target = frameNumber / project.settings.fps;
  if (!Number.isFinite(target)) return;
  if (audio.readyState < HTMLMediaElement.HAVE_METADATA) {
    audio.dataset.pendingSeek = String(target);
    return;
  }
  try { audio.currentTime = Math.min(target, Number.isFinite(audio.duration) ? audio.duration : target); }
  catch { /* The browser applies the seek when media metadata becomes available. */ }
}

function updateTimelineSummary() {
  const tracks = projectTracks();
  const clipCount = tracks.reduce((total, track) => total + (track.clips?.length ?? 0), 0);
  document.querySelector('#timeline-summary').textContent =
    `${tracks.length} track${tracks.length === 1 ? '' : 's'} · ${clipCount} clip${clipCount === 1 ? '' : 's'} · ${(project.events ?? []).length} event${(project.events ?? []).length === 1 ? '' : 's'} · ${project.settings.duration} frames`;
}

function timelineClipLabel(clip) {
  return `${clip.composition} · ${clip.start}–${clip.start + clip.duration}`;
}

function selectTimelineTrack(trackId) {
  selectedTrackId = trackId;
  selectedClipId = null;
  selectedTimeEventId = null;
  renderTimeline();
}

function selectTimelineClip(trackId, clipId) {
  selectedTrackId = trackId;
  selectedClipId = clipId;
  selectedTimeEventId = null;
  for (const block of timelineCanvas.querySelectorAll('.clip-block')) {
    block.classList.toggle('selected',
      block.dataset.trackId === trackId && block.dataset.clipId === clipId);
  }
  for (const label of timelineCanvas.querySelectorAll('.timeline-track-label')) {
    label.classList.toggle('selected', label.dataset.trackId === trackId);
  }
  updateClipInspector();
  updateTimeEventInspector();
}

function selectTimelineEvent(eventId) {
  selectedTimeEventId = eventId;
  selectedClipId = null;
  for (const marker of timelineCanvas.querySelectorAll('.time-event-marker')) {
    marker.classList.toggle('selected', marker.dataset.eventId === eventId);
  }
  updateClipInspector();
  updateTimeEventInspector();
}

function positionTimelinePlayhead() {
  const playhead = timelineCanvas.querySelector('.timeline-playhead');
  if (!playhead) return;
  playhead.style.left = `${timelineLabelWidth + frame * timelineScale}px`;
}

function timelineEventLabelLayout(events, timelineWidth) {
  const rowEnds = [];
  const placements = new Map();
  const ordered = [...events].sort((left, right) => left.frame - right.frame);
  for (const event of ordered) {
    const width = Math.max(58, Math.min(170, event.id.length * 7 + 14));
    const left = Math.min(event.frame * timelineScale, Math.max(0, timelineWidth - width));
    let row = rowEnds.findIndex((end) => end + 6 <= left);
    if (row < 0) row = rowEnds.length;
    rowEnds[row] = left + width;
    placements.set(event, { width, left, row });
  }
  return { placements, rowCount: Math.max(1, rowEnds.length) };
}

function createTimelineEventRow(timelineWidth) {
  const events = Array.isArray(project.events) ? project.events : [];
  const { placements, rowCount } = timelineEventLabelLayout(events, timelineWidth);
  const height = Math.max(52, 8 + rowCount * 22);
  const row = document.createElement('div');
  row.className = 'timeline-row timeline-event-row';
  row.style.height = `${height}px`;
  const label = document.createElement('div');
  label.className = 'timeline-sticky-label timeline-event-label';
  label.textContent = 'Events';
  const lane = document.createElement('div');
  lane.className = 'timeline-event-lane';
  lane.style.width = `${timelineWidth}px`;
  lane.style.height = `${height}px`;
  for (const event of events) {
    const placement = placements.get(event);
    const line = document.createElement('div');
    line.className = 'time-event-line';
    line.dataset.eventId = event.id;
    line.style.left = `${event.frame * timelineScale}px`;
    lane.append(line);
    const marker = document.createElement('button');
    marker.type = 'button';
    marker.className = 'time-event-marker';
    marker.classList.toggle('selected', event.id === selectedTimeEventId);
    marker.dataset.eventId = event.id;
    marker.style.left = `${placement.left}px`;
    marker.style.top = `${4 + placement.row * 22}px`;
    marker.style.width = `${placement.width}px`;
    marker.title = `${event.id} · frame ${event.frame}`;
    marker.setAttribute('aria-label', `Time event ${event.id}, frame ${event.frame}`);
    const markerLabel = document.createElement('span');
    markerLabel.textContent = event.id;
    marker.append(markerLabel);
    marker.addEventListener('pointerdown', (pointerEvent) =>
      beginTimelineEventEdit(pointerEvent, lane, marker, event));
    lane.append(marker);
  }
  row.append(label, lane);
  return row;
}

function renderTimeline() {
  const duration = Math.max(1, project.settings.duration);
  const timelineWidth = duration * timelineScale;
  const tracks = projectTracks();
  timelineCanvas.style.setProperty('--timeline-width', `${timelineWidth}px`);
  timelineCanvas.style.setProperty('--frame-width', `${timelineScale}px`);
  timelineCanvas.replaceChildren();

  const rulerRow = document.createElement('div');
  rulerRow.className = 'timeline-row timeline-ruler-row';
  const rulerLabel = document.createElement('div');
  rulerLabel.className = 'timeline-sticky-label';
  rulerLabel.textContent = 'Frame';
  const ruler = document.createElement('div');
  ruler.className = 'timeline-ruler-track';
  const tickStep = timelineScale >= 12 ? 5 : timelineScale >= 6 ? 10 : timelineScale >= 3 ? 15 : 30;
  const tickFrames = new Set([duration]);
  for (let tick = 0; tick < duration; tick += tickStep) tickFrames.add(tick);
  for (const tickFrame of [...tickFrames].sort((left, right) => left - right)) {
    const tick = document.createElement('div');
    tick.className = 'timeline-ruler-tick';
    tick.style.left = `${tickFrame * timelineScale}px`;
    const label = document.createElement('span');
    label.textContent = String(tickFrame);
    tick.append(label);
    ruler.append(tick);
  }
  rulerRow.append(rulerLabel, ruler);
  timelineCanvas.append(rulerRow);
  timelineCanvas.append(createTimelineEventRow(timelineWidth));

  if (tracks.length === 0) {
    const emptyRow = document.createElement('div');
    emptyRow.className = 'timeline-row timeline-track-row';
    const emptyLabel = document.createElement('div');
    emptyLabel.className = 'timeline-sticky-label';
    emptyLabel.textContent = 'No tracks';
    const emptyLane = document.createElement('div');
    emptyLane.className = 'timeline-empty';
    emptyLane.style.width = `${timelineWidth}px`;
    emptyLane.textContent = 'Add a track, then add a composition clip.';
    emptyRow.append(emptyLabel, emptyLane);
    timelineCanvas.append(emptyRow);
  }

  for (const [trackIndex, track] of tracks.entries()) {
    if (!Array.isArray(track.clips)) track.clips = [];
    const clipRows = [];
    const clipRowByClip = new Map();
    for (const clip of track.clips) {
      let rowIndex = clipRows.findIndex((row) => row.every((other) =>
        clip.start + clip.duration <= other.start ||
        clip.start >= other.start + other.duration));
      if (rowIndex < 0) {
        rowIndex = clipRows.length;
        clipRows.push([]);
      }
      clipRows[rowIndex].push(clip);
      clipRowByClip.set(clip, rowIndex);
    }
    const trackHeight = Math.max(54, 14 + clipRows.length * 40);
    const row = document.createElement('div');
    row.className = 'timeline-row timeline-track-row';
    row.style.height = `${trackHeight}px`;
    const label = document.createElement('button');
    label.type = 'button';
    label.className = 'timeline-sticky-label timeline-track-label';
    label.classList.toggle('selected', track.id === selectedTrackId);
    label.dataset.trackId = track.id;
    label.setAttribute('aria-label', `Select track ${track.id}`);
    const name = document.createElement('span');
    name.textContent = track.id || `Track ${trackIndex + 1}`;
    const count = document.createElement('small');
    count.textContent = String(track.clips.length);
    label.append(name, count);
    label.addEventListener('click', () => selectTimelineTrack(track.id));

    const lane = document.createElement('div');
    lane.className = 'timeline-lane';
    lane.dataset.trackId = track.id;
    lane.style.width = `${timelineWidth}px`;
    lane.style.height = `${trackHeight}px`;
    for (const clip of track.clips) {
      const block = document.createElement('button');
      block.type = 'button';
      block.className = 'clip-block';
      block.classList.toggle('selected', track.id === selectedTrackId && clip.id === selectedClipId);
      block.dataset.trackId = track.id;
      block.dataset.clipId = clip.id;
      block.style.left = `${clip.start * timelineScale}px`;
      block.style.top = `${8 + clipRowByClip.get(clip) * 40}px`;
      block.style.width = `${Math.max(timelineScale, clip.duration * timelineScale)}px`;
      block.title = `${timelineClipLabel(clip)} frames`;
      block.setAttribute('aria-label', `${clip.composition}, starts at frame ${clip.start}, duration ${clip.duration} frames`);
      const labelText = document.createElement('span');
      labelText.className = 'clip-block-label';
      labelText.textContent = timelineClipLabel(clip);
      const startHandle = document.createElement('span');
      startHandle.className = 'clip-trim-handle clip-trim-start';
      startHandle.dataset.trimEdge = 'start';
      startHandle.setAttribute('aria-hidden', 'true');
      const endHandle = document.createElement('span');
      endHandle.className = 'clip-trim-handle clip-trim-end';
      endHandle.dataset.trimEdge = 'end';
      endHandle.setAttribute('aria-hidden', 'true');
      block.append(labelText, startHandle, endHandle);
      block.addEventListener('pointerdown', (event) => beginTimelineClipEdit(event, lane, block, track, clip));
      lane.append(block);
    }
    row.append(label, lane);
    timelineCanvas.append(row);
  }

  const playhead = document.createElement('div');
  playhead.className = 'timeline-playhead';
  timelineCanvas.append(playhead);
  document.querySelector('#zoom-level').textContent = `${timelineScale} px/frame`;
  document.querySelector('#zoom-out').disabled = timelineScale === timelineZoomStops[0];
  document.querySelector('#zoom-in').disabled = timelineScale === timelineZoomStops[timelineZoomStops.length - 1];
  updateTimelineSummary();
  updateClipInspector();
  updateTimeEventInspector();
  updateVoiceOverControls();
  positionTimelinePlayhead();
}

function timelineSnapPoints(excludedClip) {
  const points = [0, frame, project.settings.duration];
  for (const track of projectTracks()) {
    for (const clip of track.clips ?? []) {
      if (clip === excludedClip) continue;
      points.push(clip.start, clip.start + clip.duration);
    }
  }
  return points;
}

function nearestTimelineSnap(value, points) {
  if (!document.querySelector('#timeline-snap').checked) return value;
  const tolerance = Math.max(1, Math.ceil(8 / timelineScale));
  let result = value;
  let distance = tolerance + 1;
  for (const point of points) {
    const nextDistance = Math.abs(point - value);
    if (nextDistance <= tolerance && nextDistance < distance) {
      result = point;
      distance = nextDistance;
    }
  }
  return result;
}

function beginTimelineEventEdit(pointerEvent, lane, marker, selectedEvent) {
  pointerEvent.preventDefault();
  selectTimelineEvent(selectedEvent.id);
  const events = project.events ?? (project.events = []);
  const pointerFrame = Math.round((pointerEvent.clientX - lane.getBoundingClientRect().left) / timelineScale);
  const originalFrames = events.map((event) => event.frame);
  const originalFrame = selectedEvent.frame;
  const snapPoints = [
    ...timelineSnapPoints(null),
    ...events.filter((event) => event !== selectedEvent).map((event) => event.frame),
  ];
  let changed = false;
  let lastMoveOnly = pointerEvent.altKey;

  function onPointerMove(moveEvent) {
    const currentPointerFrame = Math.round((moveEvent.clientX - lane.getBoundingClientRect().left) / timelineScale);
    const target = nearestTimelineSnap(originalFrame + currentPointerFrame - pointerFrame, snapPoints);
    const onlySelected = pointerEvent.altKey || moveEvent.altKey;
    lastMoveOnly = onlySelected;
    if (onlySelected) {
      selectedEvent.frame = Math.max(0, Math.min(target, project.settings.duration - 1));
    } else {
      const affectedIndexes = originalFrames
        .map((original, index) => original >= originalFrame ? index : -1)
        .filter((index) => index >= 0);
      const affectedFrames = affectedIndexes.map((index) => originalFrames[index]);
      const minDelta = Math.max(...affectedFrames.map((value) => -value));
      const maxDelta = Math.min(...affectedFrames.map((value) => project.settings.duration - 1 - value));
      const delta = Math.max(minDelta, Math.min(maxDelta, target - originalFrame));
      for (const index of affectedIndexes) {
        events[index].frame = originalFrames[index] + delta;
      }
    }
    changed ||= events.some((event, index) => event.frame !== originalFrames[index]);
    for (const event of events) {
      const line = lane.querySelector(`.time-event-line[data-event-id="${CSS.escape(event.id)}"]`);
      if (line) line.style.left = `${event.frame * timelineScale}px`;
      const eventMarker = lane.querySelector(`.time-event-marker[data-event-id="${CSS.escape(event.id)}"]`);
      if (eventMarker) {
        eventMarker.style.left = `${Math.min(event.frame * timelineScale, Math.max(0, lane.clientWidth - eventMarker.offsetWidth))}px`;
        eventMarker.title = `${event.id} · frame ${event.frame}`;
        eventMarker.setAttribute('aria-label', `Time event ${event.id}, frame ${event.frame}`);
      }
    }
    document.querySelector('#time-event-frame').value = selectedEvent.frame;
  }

  function finishPointerEdit() {
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', finishPointerEdit);
    window.removeEventListener('pointercancel', finishPointerEdit);
    if (changed) {
      renderTimeline();
      setFrame(frame);
      showMessage(lastMoveOnly ? 'Time event moved independently' : 'Time event and later events retimed');
    }
  }

  window.addEventListener('pointermove', onPointerMove);
  window.addEventListener('pointerup', finishPointerEdit, { once: true });
  window.addEventListener('pointercancel', finishPointerEdit, { once: true });
}

function beginTimelineClipEdit(event, lane, block, track, clip) {
  event.preventDefault();
  selectTimelineClip(track.id, clip.id);
  const trimEdge = event.target.closest('.clip-trim-handle')?.dataset.trimEdge;
  const mode = trimEdge === 'start' ? 'trim-start' : trimEdge === 'end' ? 'trim-end' : 'move';
  const startPointerFrame = Math.round((event.clientX - lane.getBoundingClientRect().left) / timelineScale);
  const originalStart = clip.start;
  const originalDuration = clip.duration;
  const originalEnd = originalStart + originalDuration;
  const snapPoints = timelineSnapPoints(clip);
  let changed = false;

  function updateBlock() {
    block.style.left = `${clip.start * timelineScale}px`;
    block.style.width = `${Math.max(timelineScale, clip.duration * timelineScale)}px`;
    block.title = `${timelineClipLabel(clip)} frames`;
    block.setAttribute('aria-label', `${clip.composition}, starts at frame ${clip.start}, duration ${clip.duration} frames`);
    block.querySelector('.clip-block-label').textContent = timelineClipLabel(clip);
    updateClipInspector();
    updateTimelineSummary();
  }

  function onPointerMove(moveEvent) {
    const pointerFrame = Math.round((moveEvent.clientX - lane.getBoundingClientRect().left) / timelineScale);
    const delta = pointerFrame - startPointerFrame;
    if (mode === 'move') {
      const candidates = snapPoints.flatMap((point) => [point, point - originalDuration]);
      const snapped = nearestTimelineSnap(originalStart + delta, candidates);
      clip.start = Math.max(0, Math.min(snapped, project.settings.duration - originalDuration));
      clip.duration = originalDuration;
    } else if (mode === 'trim-start') {
      const snapped = nearestTimelineSnap(originalStart + delta, snapPoints);
      clip.start = Math.max(0, Math.min(snapped, originalEnd - 1));
      clip.duration = originalEnd - clip.start;
    } else {
      const snapped = nearestTimelineSnap(originalEnd + delta, snapPoints);
      const end = Math.max(originalStart + 1, Math.min(snapped, project.settings.duration));
      clip.start = originalStart;
      clip.duration = end - originalStart;
    }
    changed ||= clip.start !== originalStart || clip.duration !== originalDuration;
    updateBlock();
  }

  function finishPointerEdit() {
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', finishPointerEdit);
    window.removeEventListener('pointercancel', finishPointerEdit);
    if (changed) {
      renderTimeline();
      setFrame(frame);
      showMessage('Timeline clip updated');
    }
  }

  window.addEventListener('pointermove', onPointerMove);
  window.addEventListener('pointerup', finishPointerEdit, { once: true });
  window.addEventListener('pointercancel', finishPointerEdit, { once: true });
}

function updateSelectedClip(mutator) {
  const entry = selectedClipEntry();
  if (!entry) return;
  mutator(entry.clip);
  renderTimeline();
  setFrame(frame);
  showMessage('Timeline clip updated');
}

function cloneProjectValue(value) {
  return JSON.parse(JSON.stringify(value ?? {}));
}

function changeTimelineZoom(direction) {
  const currentIndex = timelineZoomStops.indexOf(timelineScale);
  const nextIndex = Math.max(0, Math.min(timelineZoomStops.length - 1, currentIndex + direction));
  if (nextIndex === currentIndex) return;
  const centerFrame = Math.max(0,
    (timelineScroll.scrollLeft + timelineScroll.clientWidth / 2 - timelineLabelWidth) / timelineScale);
  timelineScale = timelineZoomStops[nextIndex];
  renderTimeline();
  timelineScroll.scrollLeft = Math.max(0,
    timelineLabelWidth + centerFrame * timelineScale - timelineScroll.clientWidth / 2);
}

function showMessage(message) {
  document.querySelector('#message').textContent = message;
}

function setFrame(nextFrame) {
  frame = Math.max(0, Math.min(Math.round(nextFrame), project.settings.duration - 1));
  document.querySelector('#timeline-slider').max = project.settings.duration - 1;
  document.querySelector('#timeline-slider').value = frame;
  document.querySelector('#timeline-frame').textContent = `${frame} / ${project.settings.duration - 1}`;
  positionTimelinePlayhead();
  if (!playing) seekVoiceOver(frame);
  renderFrame({
    composition: project.composition,
    frame,
    fps: project.settings.fps,
    width: project.settings.width,
    height: project.settings.height,
    durationInFrames: project.settings.duration,
    props: project.props,
    assets: project.assets.map((asset) => asset.path),
    timeline: project.tracks.flatMap((track) => track.clips),
    time_events: project.events ?? [],
  }).catch((error) => showMessage(`preview error: ${error}`));
}

document.querySelector('#play-toggle').addEventListener('click', (event) => {
  playing = !playing;
  const audio = document.querySelector('#voice-over-player');
  if (playing) {
    playbackStartedAt = performance.now() - (frame * 1000 / project.settings.fps);
    if (voiceOverAsset()) {
      seekVoiceOver(frame);
      audio.play().catch((error) => showMessage(`voice-over playback error: ${error.message}`));
    }
  } else {
    audio.pause();
  }
  event.currentTarget.textContent = playing ? 'Pause' : 'Play';
});
document.querySelector('#timeline-slider').addEventListener('input', (event) => {
  playing = false;
  document.querySelector('#voice-over-player').pause();
  lastPlaybackFrame = -1;
  document.querySelector('#play-toggle').textContent = 'Play';
  setFrame(Number(event.currentTarget.value));
});

document.querySelector('#add-track').addEventListener('click', () => {
  const tracks = projectTracks();
  let trackNumber = tracks.length + 1;
  while (tracks.some((track) => track.id === `Track ${trackNumber}`)) trackNumber += 1;
  const track = { id: `Track ${trackNumber}`, clips: [] };
  tracks.push(track);
  selectedTrackId = track.id;
  selectedClipId = null;
  selectedTimeEventId = null;
  renderTimeline();
  showMessage(`${track.id} added`);
});

document.querySelector('#add-time-event').addEventListener('click', () => {
  const events = Array.isArray(project.events) ? project.events : (project.events = []);
  let number = events.length + 1;
  while (events.some((event) => event.id === `event-${number}`)) number += 1;
  const event = { id: `event-${number}`, frame };
  events.push(event);
  selectedTimeEventId = event.id;
  selectedClipId = null;
  renderTimeline();
  setFrame(frame);
  showMessage(`${event.id} added at frame ${frame}`);
});

document.querySelector('#time-event-name').addEventListener('change', (inputEvent) => {
  const event = selectedTimeEvent();
  if (!event) return;
  const id = inputEvent.currentTarget.value.trim();
  if (!id || project.events.some((item) => item !== event && item.id === id)) {
    updateTimeEventInspector();
    showMessage(!id ? 'Event name cannot be empty' : `Event "${id}" already exists`);
    return;
  }
  event.id = id;
  selectedTimeEventId = id;
  renderTimeline();
  setFrame(frame);
  showMessage(`Event renamed to ${id}`);
});

document.querySelector('#time-event-frame').addEventListener('change', (inputEvent) => {
  const event = selectedTimeEvent();
  if (!event) return;
  const value = Number.parseInt(inputEvent.currentTarget.value, 10);
  if (!Number.isFinite(value)) return updateTimeEventInspector();
  event.frame = Math.max(0, Math.min(value, project.settings.duration - 1));
  renderTimeline();
  setFrame(frame);
  showMessage(`${event.id} moved to frame ${event.frame}`);
});

document.querySelector('#remove-time-event').addEventListener('click', () => {
  const event = selectedTimeEvent();
  if (!event) return;
  project.events = project.events.filter((item) => item !== event);
  selectedTimeEventId = null;
  renderTimeline();
  setFrame(frame);
  showMessage(`${event.id} removed`);
});

document.querySelector('#voice-over-asset').addEventListener('change', (changeEvent) => {
  project.voice_over_asset_id = changeEvent.currentTarget.value || null;
  updateVoiceOverControls();
  showMessage(project.voice_over_asset_id ? 'Voice-over selected' : 'Voice-over cleared');
});

document.querySelector('#import-voice-over').addEventListener('click', async () => {
  try {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Audio', extensions: ['aac', 'flac', 'm4a', 'mp3', 'ogg', 'wav', 'webm'] }],
    });
    if (!selected || Array.isArray(selected)) return;
    if (!Array.isArray(project.assets)) project.assets = [];
    const asset = {
      id: makeProjectId('voice-over'),
      path: selected,
      kind: 'audio',
      sha256: null,
    };
    project.assets.push(asset);
    project.voice_over_asset_id = asset.id;
    updateVoiceOverControls();
    showMessage(`Voice-over imported: ${asset.id}`);
  } catch (error) {
    showMessage(`voice-over import error: ${error}`);
  }
});

document.querySelector('#add-clip').addEventListener('click', () => {
  const tracks = projectTracks();
  let track = tracks.find((item) => item.id === selectedTrackId) ?? tracks[0];
  if (!track) {
    let trackNumber = 1;
    while (tracks.some((item) => item.id === `Track ${trackNumber}`)) trackNumber += 1;
    track = { id: `Track ${trackNumber}`, clips: [] };
    tracks.push(track);
  }
  if (!Array.isArray(track.clips)) track.clips = [];
  const duration = Math.min(30, project.settings.duration);
  const start = Math.min(frame, project.settings.duration - duration);
  const clip = {
    id: makeProjectId('clip'),
    composition: project.composition,
    start,
    duration,
    props: cloneProjectValue(project.props),
  };
  track.clips.push(clip);
  selectedTrackId = track.id;
  selectedClipId = clip.id;
  selectedTimeEventId = null;
  renderTimeline();
  setFrame(frame);
  showMessage('Composition clip added');
});

document.querySelector('#duplicate-clip').addEventListener('click', () => {
  const entry = selectedClipEntry();
  if (!entry) return;
  const { track, clip } = entry;
  const duplicate = {
    ...clip,
    id: makeProjectId('clip'),
    start: Math.min(clip.start + clip.duration, project.settings.duration - clip.duration),
    props: cloneProjectValue(clip.props),
  };
  const index = track.clips.indexOf(clip);
  track.clips.splice(index + 1, 0, duplicate);
  selectedClipId = duplicate.id;
  selectedTimeEventId = null;
  renderTimeline();
  setFrame(frame);
  showMessage('Composition clip duplicated');
});

document.querySelector('#remove-clip').addEventListener('click', () => {
  const entry = selectedClipEntry();
  if (!entry) return;
  entry.track.clips = entry.track.clips.filter((clip) => clip !== entry.clip);
  selectedClipId = null;
  renderTimeline();
  setFrame(frame);
  showMessage('Composition clip removed');
});

document.querySelector('#clip-composition').addEventListener('change', (event) => {
  const composition = event.currentTarget.value.trim();
  if (!composition) {
    updateClipInspector();
    showMessage('Composition ID cannot be empty');
    return;
  }
  updateSelectedClip((clip) => { clip.composition = composition; });
});

document.querySelector('#clip-start').addEventListener('change', (event) => {
  const value = Number.parseInt(event.currentTarget.value, 10);
  if (!Number.isFinite(value)) return updateClipInspector();
  updateSelectedClip((clip) => {
    clip.start = Math.max(0, Math.min(value, project.settings.duration - clip.duration));
  });
});

document.querySelector('#clip-duration').addEventListener('change', (event) => {
  const value = Number.parseInt(event.currentTarget.value, 10);
  if (!Number.isFinite(value)) return updateClipInspector();
  updateSelectedClip((clip) => {
    clip.duration = Math.max(1, Math.min(value, project.settings.duration - clip.start));
  });
});

document.querySelector('#zoom-out').addEventListener('click', () => changeTimelineZoom(-1));
document.querySelector('#zoom-in').addEventListener('click', () => changeTimelineZoom(1));

renderTimeline();

function expectedOutputFrameCount(job) {
  const output = String(job.output ?? '').split(/[\\/]/).pop() ?? '';
  const extension = output.split('.').pop()?.toLowerCase();
  if (['png', 'jpg', 'jpeg', 'webp'].includes(extension)) return 1;

  const settings = job.project.settings;
  const start = settings.frame_start ?? 0;
  const end = settings.frame_end ?? settings.duration - 1;
  const step = Math.max(1, settings.frame_step ?? 1);
  return Math.ceil(Math.max(0, end - start + 1) / step);
}

async function refreshJob(id) {
  const job = await invoke('get_render_job', { id });
  if (!job) return;
  const totalFrames = expectedOutputFrameCount(job);
  const progress = `render ${job.completed_frames}/${totalFrames} · encode ${job.encoded_frames ?? 0}/${totalFrames}`;
  document.querySelector('#job').textContent = `${job.id} · ${job.status} · ${progress}`;
  const terminal = ['completed', 'failed', 'cancelled'].includes(job.status);
  document.querySelector('#cancel-render').disabled = terminal;
  if (terminal && job.error) showMessage(job.error);
}

async function refreshJobList() {
  const jobs = await invoke('list_render_jobs');
  const list = document.querySelector('#job-list');
  if (!jobs.length) { list.textContent = 'No jobs'; return; }
  list.replaceChildren(...jobs.map((job) => {
  const item = document.createElement('div');
    item.className = 'job-item';
    const totalFrames = expectedOutputFrameCount(job);
    const progress = `render ${job.completed_frames}/${totalFrames} · encode ${job.encoded_frames ?? 0}/${totalFrames}`;
    const diagnostics = job.gpu_frames !== null && job.gpu_frames !== undefined
      ? ` · GPU ${job.gpu_frames} · CPU fallback ${job.cpu_fallback_frames ?? 0}${job.gpu_fallback_reason ? ` (${job.gpu_fallback_reason})` : ''}`
      : '';
    const label = document.createElement('span');
    label.textContent = `${job.id} · ${job.status} · ${progress}${diagnostics}${job.error ? ` · ${job.error}` : ''}`;
    item.append(label);
    if (['failed', 'cancelled'].includes(job.status)) {
      const retry = document.createElement('button');
      retry.className = 'retry-job';
      retry.textContent = 'Retry';
      retry.addEventListener('click', async () => {
        try {
          currentJobId = await invoke('retry_render_job', { id: job.id });
          const output = await join(await tempDir(), `dioxuscut-${currentJobId}.mp4`);
          await invoke('start_render_job', { id: currentJobId, output });
          showMessage(`${currentJobId} rendering → ${output}`);
          await refreshJobList();
        } catch (error) { showMessage(`retry error: ${error}`); }
      });
      item.append(' ', retry);
    }
    return item;
  }));
}

setInterval(() => {
  if (currentJobId) refreshJob(currentJobId).catch((error) => showMessage(`job error: ${error}`));
  refreshJobList().catch((error) => showMessage(`queue error: ${error}`));
}, 250);

document.querySelector('#load-project').addEventListener('click', async () => {
  try {
    const selected = await open({ filters: [{ name: 'Dioxuscut project', extensions: ['json'] }] });
    if (!selected || Array.isArray(selected)) return;
    document.querySelector('#project-path').value = selected;
    project = await invoke('load_project', { path: selected });
    selectedTrackId = project.tracks?.[0]?.id ?? null;
    selectedClipId = null;
    selectedTimeEventId = null;
    playing = false;
    document.querySelector('#play-toggle').textContent = 'Play';
    document.querySelector('#voice-over-player').pause();
    renderTimeline();
    playbackStartedAt = performance.now();
    lastPlaybackFrame = -1;
    showMessage(`loaded ${project.composition}`);
    setFrame(frame);
  } catch (error) { showMessage(`load error: ${error}`); }
});

document.querySelector('#save-project').addEventListener('click', async () => {
  try {
    await ensureThree();
    project.props = {
      ...(project.props && typeof project.props === 'object' && !Array.isArray(project.props)
        ? project.props
        : {}),
      color: cube.material.color.getStyle(),
    };
    const selected = await save({
      defaultPath: document.querySelector('#project-path').value,
      filters: [{ name: 'Dioxuscut project', extensions: ['json'] }],
    });
    if (!selected) return;
    document.querySelector('#project-path').value = selected;
    await invoke('save_project', { path: selected, project });
    showMessage('project saved');
  } catch (error) { showMessage(`save error: ${error}`); }
});

document.querySelector('#submit-render').addEventListener('click', async () => {
  try {
    const id = await invoke('submit_project', { project });
    const output = await join(await tempDir(), `dioxuscut-${id}.mp4`);
    await invoke('start_render_job', { id, output });
    currentJobId = id;
    document.querySelector('#cancel-render').disabled = false;
    document.querySelector('#cancel-render').dataset.jobId = id;
    showMessage(`${id} rendering → ${output}`);
    await refreshJob(id);
    await refreshJobList();
  } catch (error) { showMessage(`queue error: ${error}`); }
});

document.querySelector('#cancel-render').addEventListener('click', async (event) => {
  const id = event.currentTarget.dataset.jobId;
  if (!id) return;
  try {
    await invoke('cancel_render_job', { id });
    await refreshJob(id);
    await refreshJobList();
    showMessage('render cancelled');
  } catch (error) { showMessage(`cancel error: ${error}`); }
});

function renderPreview() {
  if (window.__DIOXUSCUT_HEADLESS_RENDER__) return;
  if (playing) {
    const elapsed = Math.max(0, performance.now() - playbackStartedAt);
    const duration = Math.max(project.settings.duration, 1);
    const nextFrame = Math.floor(elapsed * project.settings.fps / 1000) % duration;
    if (nextFrame !== lastPlaybackFrame) {
      lastPlaybackFrame = nextFrame;
      setFrame(nextFrame);
    }
  }
  requestAnimationFrame(renderPreview);
}
renderPreview();

Promise.all([invoke('backend_capabilities'), invoke('web_worker_protocol')])
  .then(([capabilities, protocol]) => {
    document.querySelector('#backend').textContent = capabilities.browser_runtime
      ? 'Tauri · Three.js preview' : 'native preview';
    document.querySelector('#protocol').textContent = `worker protocol v${protocol.version}`;
  })
  .catch((error) => {
    document.querySelector('#backend').textContent = `bridge error: ${error}`;
  });

invoke('validate_frame_request', {
  request: { frame: 0, fps: 30, width: 1280, height: 720, props: {} },
}).catch((error) => { document.querySelector('#protocol').textContent = `protocol error: ${error}`; });
