import test from 'node:test';
import assert from 'node:assert/strict';
import {
  getCompositions,
  selectComposition,
  renderMedia,
  renderStill,
} from '../dist/renderer.mjs';

test('renderer: getCompositions & selectComposition', async () => {
  const comps = await getCompositions();
  assert.ok(Array.isArray(comps));
  assert.ok(comps.length > 0);

  const selected = await selectComposition({ id: 'HelloWorld' });
  assert.equal(selected.id, 'HelloWorld');
  assert.equal(selected.fps, 30);
});

test('renderer: renderMedia with progress tracking', async () => {
  let progressReported = false;
  const result = await renderMedia({
    composition: 'HelloWorld',
    outputLocation: '/tmp/test_render_output.mp4',
    codec: 'h264',
    onProgress: (progress) => {
      progressReported = true;
      assert.ok(progress.progress >= 0 && progress.progress <= 1);
    },
  });

  assert.equal(result.outputLocation, '/tmp/test_render_output.mp4');
  assert.ok(result.totalFrames > 0);
  assert.ok(progressReported);
});

test('renderer: renderStill snapshot', async () => {
  const result = await renderStill({
    composition: 'HelloWorld',
    output: '/tmp/test_snapshot.png',
    frame: 15,
    imageFormat: 'png',
  });

  assert.equal(result.output, '/tmp/test_snapshot.png');
  assert.equal(result.frame, 15);
  assert.equal(result.imageFormat, 'png');
});
