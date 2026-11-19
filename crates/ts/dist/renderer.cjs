/**
 * @rust-motion/remotion/renderer — CommonJS build. Bridge to the native `rust-motion` CLI.
 *
 * Rendering is delegated to the `rust-motion` binary (set RUST_MOTION_BIN to use a
 * specific path). The binary renders compositions registered in its native (Rust)
 * registry; TypeScript component functions are NOT evaluated by the native renderer.
 * Every failure (missing binary, unknown composition, non-zero exit, missing output)
 * rejects the returned promise.
 */

const { spawn } = require('node:child_process');
const { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { dirname, join, resolve } = require('node:path');
const { getRegisteredCompositions, getComposition } = require('./index.cjs');

/** Defaults used by `rust-motion render` when a flag is not passed. */
const CLI_DEFAULTS = { width: 1920, height: 1080, fps: 30, durationInFrames: 150 };

const STILL_CODECS = { png: 'png', jpeg: 'jpeg', jpg: 'jpeg', webp: 'webp' };

function binPath() {
  return process.env.RUST_MOTION_BIN || 'rust-motion';
}

function tail(text, lines = 6) {
  return text.trim().split('\n').slice(-lines).join('\n');
}

/**
 * Run the CLI and resolve with { stdout, stderr } on exit code 0.
 * Rejects if the binary cannot be started or exits non-zero.
 */
function runCli(args, { onStderr } = {}) {
  return new Promise((resolvePromise, rejectPromise) => {
    let child;
    try {
      child = spawn(binPath(), args, { stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (err) {
      rejectPromise(new Error(`failed to start rust-motion (${binPath()}): ${err.message}`));
      return;
    }

    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => {
      stdout += d.toString();
    });
    child.stderr.on('data', (d) => {
      const text = d.toString();
      stderr += text;
      if (onStderr) onStderr(text);
    });

    child.on('error', (err) => {
      rejectPromise(
        new Error(
          `could not run the rust-motion binary "${binPath()}": ${err.message}. ` +
            'Install it or set RUST_MOTION_BIN to its path.',
        ),
      );
    });

    child.on('close', (code, signal) => {
      if (code === 0) {
        resolvePromise({ stdout, stderr });
      } else {
        const how = signal ? `was terminated by ${signal}` : `exited with code ${code}`;
        rejectPromise(new Error(`rust-motion ${how}:\n${tail(stderr)}`));
      }
    });
  });
}

/** Names of compositions known to the native registry (empty if the CLI is unavailable). */
async function listNativeCompositions() {
  try {
    const { stdout } = await runCli(['list-compositions', '--json']);
    const parsed = JSON.parse(stdout);
    return Array.isArray(parsed.compositions) ? parsed.compositions : [];
  } catch {
    return [];
  }
}

function describe(comp) {
  return {
    id: comp.id,
    width: comp.width,
    height: comp.height,
    fps: comp.fps,
    durationInFrames: comp.durationInFrames,
    defaultProps: comp.defaultProps,
  };
}

function resolveMeta(composition) {
  const id = typeof composition === 'string' ? composition : composition?.id;
  if (!id) {
    throw new Error('composition id is required');
  }
  const meta = typeof composition === 'object' ? composition : (getComposition(id) ?? {});
  return { id, meta };
}

function withTsHint(error, id) {
  if (getComposition(id)) {
    error.message +=
      `\nNote: "${id}" is registered from TypeScript, but the native renderer does not ` +
      'evaluate TypeScript components. Only compositions in the Rust registry can be rendered.';
  }
  return error;
}

function writeProps(inputProps) {
  if (!inputProps || Object.keys(inputProps).length === 0) return null;
  const dir = mkdtempSync(join(tmpdir(), 'rust-motion-props-'));
  const file = join(dir, 'props.json');
  writeFileSync(file, JSON.stringify(inputProps));
  return { dir, file };
}

async function getCompositions(entryPoint) {
  const registered = getRegisteredCompositions();
  if (registered.length > 0) {
    return registered.map(describe);
  }
  const native = await listNativeCompositions();
  return native.map((id) => ({ id, ...CLI_DEFAULTS, defaultProps: {} }));
}

async function selectComposition({ id, entryPoint } = {}) {
  if (!id) {
    throw new Error('selectComposition: id is required');
  }
  const comp = getComposition(id);
  if (comp) {
    return describe(comp);
  }
  const native = await listNativeCompositions();
  if (native.includes(id)) {
    return { id, ...CLI_DEFAULTS, defaultProps: {} };
  }
  throw new Error(
    `Composition "${id}" not found.` +
      (native.length ? ` Available native compositions: ${native.join(', ')}` : ''),
  );
}

async function renderMedia(options) {
  const {
    composition,
    outputLocation,
    codec = 'h264',
    crf = 18,
    onProgress,
    inputProps = {},
    concurrency,
    hwAccel = 'auto',
  } = options;

  if (!outputLocation) {
    throw new Error('renderMedia: outputLocation is required');
  }
  const { id, meta } = resolveMeta(composition);

  const outDir = dirname(resolve(outputLocation));
  if (!existsSync(outDir)) {
    mkdirSync(outDir, { recursive: true });
  }

  const width = meta.width ?? CLI_DEFAULTS.width;
  const height = meta.height ?? CLI_DEFAULTS.height;
  const fps = meta.fps ?? CLI_DEFAULTS.fps;
  const durationInFrames = meta.durationInFrames ?? CLI_DEFAULTS.durationInFrames;

  const args = [
    'render',
    '--composition', id,
    '--output', outputLocation,
    '--width', String(width),
    '--height', String(height),
    '--fps', String(fps),
    '--duration', String(durationInFrames),
    '--codec', codec,
    '--crf', String(crf),
    '--hw-accel', hwAccel,
  ];
  if (concurrency) {
    args.push('--concurrency', String(concurrency));
  }

  const props = writeProps(inputProps);
  if (props) {
    args.push('--props', props.file);
  }

  try {
    await runCli(args, {
      onStderr: (text) => {
        const match = text.match(/(\d+)\s*\/\s*(\d+)\s+frames/i);
        if (match && onProgress) {
          const renderedFrames = parseInt(match[1], 10);
          const totalFrames = parseInt(match[2], 10);
          onProgress({
            renderedFrames,
            encodedFrames: renderedFrames,
            totalFrames,
            progress: totalFrames > 0 ? renderedFrames / totalFrames : 1,
          });
        }
      },
    });
  } catch (err) {
    throw withTsHint(err, id);
  } finally {
    if (props) rmSync(props.dir, { recursive: true, force: true });
  }

  if (!existsSync(outputLocation)) {
    throw new Error(`rust-motion reported success but produced no file at ${outputLocation}`);
  }
  if (onProgress) {
    onProgress({
      renderedFrames: durationInFrames,
      encodedFrames: durationInFrames,
      totalFrames: durationInFrames,
      progress: 1,
    });
  }
  return { outputLocation, totalFrames: durationInFrames, width, height, fps };
}

async function renderStill(options) {
  const {
    composition,
    output,
    frame = 0,
    imageFormat = 'png',
    inputProps = {},
  } = options;

  if (!output) {
    throw new Error('renderStill: output is required');
  }
  const stillCodec = STILL_CODECS[String(imageFormat).toLowerCase()];
  if (!stillCodec) {
    throw new Error(`renderStill: unsupported imageFormat "${imageFormat}" (use png, jpeg or webp)`);
  }
  if (!Number.isInteger(frame) || frame < 0) {
    throw new Error('renderStill: frame must be a non-negative integer');
  }
  const { id, meta } = resolveMeta(composition);

  const outDir = dirname(resolve(output));
  if (!existsSync(outDir)) {
    mkdirSync(outDir, { recursive: true });
  }

  const width = meta.width ?? CLI_DEFAULTS.width;
  const height = meta.height ?? CLI_DEFAULTS.height;
  const fps = meta.fps ?? CLI_DEFAULTS.fps;
  const durationInFrames = meta.durationInFrames ?? CLI_DEFAULTS.durationInFrames;

  const args = [
    'render',
    '--composition', id,
    '--output', output,
    '--width', String(width),
    '--height', String(height),
    '--fps', String(fps),
    '--duration', String(Math.max(durationInFrames, frame + 1)),
    '--codec', stillCodec,
    '--frame-start', String(frame),
    '--frame-end', String(frame),
  ];

  const props = writeProps(inputProps);
  if (props) {
    args.push('--props', props.file);
  }

  try {
    await runCli(args);
  } catch (err) {
    throw withTsHint(err, id);
  } finally {
    if (props) rmSync(props.dir, { recursive: true, force: true });
  }

  if (!existsSync(output)) {
    throw new Error(`rust-motion reported success but produced no file at ${output}`);
  }
  return { output, frame, width, height, imageFormat: stillCodec };
}

module.exports = {
  getCompositions,
  selectComposition,
  renderMedia,
  renderStill,
};
