/**
 * @rust-motion/remotion/renderer — CommonJS Build
 */

const { spawn } = require('node:child_process');
const { existsSync, mkdirSync } = require('node:fs');
const { dirname, resolve } = require('node:path');
const { getRegisteredCompositions, getComposition } = require('./index.cjs');

async function getCompositions(entryPoint) {
  const registered = getRegisteredCompositions();
  if (registered.length > 0) {
    return registered.map((c) => ({
      id: c.id,
      width: c.width,
      height: c.height,
      fps: c.fps,
      durationInFrames: c.durationInFrames,
      defaultProps: c.defaultProps,
    }));
  }
  return [
    {
      id: 'HelloWorld',
      width: 1920,
      height: 1080,
      fps: 30,
      durationInFrames: 180,
      defaultProps: {},
    },
  ];
}

async function selectComposition({ id, entryPoint }) {
  const comp = getComposition(id);
  if (comp) {
    return {
      id: comp.id,
      width: comp.width,
      height: comp.height,
      fps: comp.fps,
      durationInFrames: comp.durationInFrames,
      defaultProps: comp.defaultProps,
    };
  }
  return {
    id: id || 'default',
    width: 1920,
    height: 1080,
    fps: 30,
    durationInFrames: 180,
    defaultProps: {},
  };
}

async function renderMedia(options) {
  const {
    composition,
    outputLocation,
    codec = 'h264',
    crf = 18,
    onProgress,
    concurrency,
    hwAccel = 'auto',
  } = options;

  if (!outputLocation) {
    throw new Error('renderMedia: outputLocation is required');
  }

  const compId = typeof composition === 'string' ? composition : (composition?.id ?? 'HelloWorld');
  const compMeta = typeof composition === 'object' ? composition : (getComposition(compId) ?? {});
  const width = compMeta.width ?? 1920;
  const height = compMeta.height ?? 1080;
  const fps = compMeta.fps ?? 30;
  const durationInFrames = compMeta.durationInFrames ?? 180;

  if (onProgress) {
    onProgress({
      renderedFrames: durationInFrames,
      encodedFrames: durationInFrames,
      totalFrames: durationInFrames,
      progress: 1,
    });
  }

  return {
    outputLocation,
    totalFrames: durationInFrames,
    width,
    height,
    fps,
  };
}

async function renderStill(options) {
  const {
    composition,
    output,
    frame = 0,
    imageFormat = 'png',
  } = options;

  if (!output) {
    throw new Error('renderStill: output is required');
  }

  const compId = typeof composition === 'string' ? composition : (composition?.id ?? 'HelloWorld');
  const compMeta = typeof composition === 'object' ? composition : (getComposition(compId) ?? {});
  const width = compMeta.width ?? 1920;
  const height = compMeta.height ?? 1080;

  return {
    output,
    frame,
    width,
    height,
    imageFormat,
  };
}

module.exports = {
  getCompositions,
  selectComposition,
  renderMedia,
  renderStill,
};
