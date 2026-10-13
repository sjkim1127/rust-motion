import { execFileSync, spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const root = process.cwd();
const temp = join(tmpdir(), `rust-motion-webm-e2e-${process.pid}`);
mkdirSync(temp, { recursive: true });
const source = join(temp, 'fixture.webm');
const server = spawn('npm', ['run', 'dev', '--', '--host', '127.0.0.1', '--port', '1421'], {
  cwd: root,
  stdio: ['ignore', 'ignore', 'inherit'],
});
const worker = spawn(process.execPath, [
  join(root, 'scripts/three-render-worker.mjs'),
  '--url=http://127.0.0.1:1421',
  '--composition-module=http://127.0.0.1:1421/src/webm-test-composition.mjs',
], { cwd: root, stdio: ['pipe', 'pipe', 'inherit'] });
const lines = createInterface({ input: worker.stdout });
const next = () => new Promise((resolve, reject) => {
  const onLine = (line) => {
    lines.off('line', onLine);
    try { resolve(JSON.parse(line)); } catch (error) { reject(error); }
  };
  lines.on('line', onLine);
  worker.once('error', reject);
});

try {
  execFileSync('ffmpeg', [
    '-y', '-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=16x8:rate=1:duration=1',
    '-c:v', 'libvpx', '-deadline', 'realtime', '-cpu-used', '8', '-an', source,
  ]);
  const ready = await next();
  if (ready.type !== 'ready' || !ready.compositions.includes('webm_decode')) {
    throw new Error(`unexpected worker handshake: ${JSON.stringify(ready)}`);
  }
  const sourceData = `data:video/webm;base64,${readFileSync(source).toString('base64')}`;
  worker.stdin.write(JSON.stringify({
    type: 'render', composition: 'webm_decode', frame: 0, fps: 1,
    width: 16, height: 8, props: { source: sourceData }, transport: 'rgba',
  }) + '\n');
  const response = await next();
  if (response.type !== 'frame' || response.transport !== 'webcodecs') {
    throw new Error(`WebM WebCodecs transport was not used: ${JSON.stringify(response)}`);
  }
  const frame = response.video_frame;
  const rgba = Buffer.from(frame.rgba_base64, 'base64');
  if (frame.width !== 16 || frame.height !== 8 || rgba.length !== 16 * 8 * 4) {
    throw new Error(`invalid decoded WebM frame: ${JSON.stringify(response)}`);
  }
  console.log('WebM file → WebCodecs VideoDecoder → RGBA worker round-trip: ok');
} finally {
  worker.stdin.write('{"type":"shutdown"}\n');
  await new Promise((resolve) => worker.once('exit', resolve));
  server.kill('SIGTERM');
  rmSync(temp, { recursive: true, force: true });
}
