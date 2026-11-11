/**
 * @rust-motion/remotion/player — Player Component & Controller Bridge
 */

import { pushVideoContext, popVideoContext } from './index.mjs';

export function Player(props) {
  const {
    component,
    durationInFrames = 180,
    compositionWidth = 1920,
    compositionHeight = 1080,
    fps = 30,
    initialFrame = 0,
    inputProps = {},
    autoPlay = false,
    loop = false,
    controls = true,
    style = {},
  } = props;

  return {
    type: 'remotion-player',
    props: {
      durationInFrames,
      compositionWidth,
      compositionHeight,
      fps,
      initialFrame,
      inputProps,
      autoPlay,
      loop,
      controls,
      style,
    },
    renderFrame(frame) {
      pushVideoContext({
        width: compositionWidth,
        height: compositionHeight,
        fps,
        durationInFrames,
        frame,
        props: inputProps,
      });
      const result = typeof component === 'function' ? component(inputProps) : null;
      popVideoContext();
      return result;
    },
  };
}

export function Thumbnail(props) {
  const { component, frame = 0, compositionWidth = 1920, compositionHeight = 1080, fps = 30, inputProps = {} } = props;
  pushVideoContext({
    width: compositionWidth,
    height: compositionHeight,
    fps,
    durationInFrames: 1,
    frame,
    props: inputProps,
  });
  const result = typeof component === 'function' ? component(inputProps) : null;
  popVideoContext();
  return result;
}
