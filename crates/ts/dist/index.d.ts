/**
 * Remotion 1:1 Compatible Type Definitions for Rust Motion
 */

export interface VideoConfig {
  width: number;
  height: number;
  fps: number;
  durationInFrames: number;
  id: string;
}

export declare function useCurrentFrame(): number;
export declare function useVideoConfig(): VideoConfig;
export declare function getInputProps<T = Record<string, unknown>>(): T;
export declare function pushVideoContext(ctx: Partial<VideoConfig> & { frame?: number; props?: Record<string, unknown> }): void;
export declare function popVideoContext(): void;

export type ExtrapolateType = 'extend' | 'clamp' | 'identity';

export interface InterpolateOptions {
  extrapolateLeft?: ExtrapolateType;
  extrapolateRight?: ExtrapolateType;
  easing?: (input: number) => number;
}

export declare function interpolate(
  input: number,
  inputRange: readonly number[],
  outputRange: readonly number[],
  options?: InterpolateOptions
): number;

export declare function interpolateColors(
  input: number,
  inputRange: readonly number[],
  outputRange: readonly string[]
): string;

export declare const Easing: {
  linear: (t: number) => number;
  ease: (t: number) => number;
  quad: (t: number) => number;
  cubic: (t: number) => number;
  poly: (n: number) => (t: number) => number;
  sin: (t: number) => number;
  circle: (t: number) => number;
  exp: (t: number) => number;
  elastic: (bounciness?: number) => (t: number) => number;
  back: (s?: number) => (t: number) => number;
  bounce: (t: number) => number;
  bezier: (x1: number, y1: number, x2: number, y2: number) => (t: number) => number;
  in: (easing: (t: number) => number) => (t: number) => number;
  out: (easing: (t: number) => number) => (t: number) => number;
  inOut: (easing: (t: number) => number) => (t: number) => number;
};

export interface SpringConfig {
  damping?: number;
  mass?: number;
  stiffness?: number;
  overshootClamping?: boolean;
}

export interface SpringOptions {
  frame: number;
  fps?: number;
  config?: SpringConfig;
  from?: number;
  to?: number;
  delay?: number;
  durationInFrames?: number;
  reverse?: boolean;
}

export declare function spring(options: SpringOptions): number;

export interface MeasureSpringOptions {
  fps?: number;
  config?: SpringConfig;
  threshold?: number;
}

export declare function measureSpring(options: MeasureSpringOptions): number;

export declare function random(seed: string | number): number;

export interface CompositionProps<T = any> {
  id: string;
  component: (props: T) => any;
  durationInFrames: number;
  fps: number;
  width: number;
  height: number;
  defaultProps?: T;
}

export declare function Composition<T = any>(props: CompositionProps<T>): null;
export declare function getRegisteredCompositions(): CompositionProps[];
export declare function getComposition(id: string): CompositionProps | undefined;

export interface SequenceProps {
  from?: number;
  durationInFrames?: number;
  layout?: 'absolute-fill' | 'none';
  style?: Record<string, any>;
  className?: string;
  children: any | ((context: { frame: number }) => any);
}

export declare function Sequence(props: SequenceProps): any;

export interface SeriesProps {
  children: any;
}

export declare function Series(props: SeriesProps): any;
export declare namespace Series {
  export const Sequence: typeof import('./index.d.ts').Sequence;
}

export interface LoopProps {
  durationInFrames: number;
  times?: number;
  children: any | ((context: { frame: number; loopIndex: number }) => any);
}

export declare function Loop(props: LoopProps): any;

export interface FreezeProps {
  frame?: number;
  children: any | ((context: { frame: number }) => any);
}

export declare function Freeze(props: FreezeProps): any;

export declare const AbsoluteFill: {
  position: 'absolute';
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  width: '100%';
  height: '100%';
  display: 'flex';
  flexDirection: 'column';
};

export interface MediaProps {
  src: string;
  volume?: number | ((frame: number) => number);
  playbackRate?: number;
  startFrom?: number;
  endAt?: number;
  [key: string]: any;
}

export declare function Audio(props: MediaProps): { type: 'audio'; props: MediaProps };
export declare function Video(props: MediaProps): { type: 'video'; props: MediaProps };
export declare function Img(props: { src: string; [key: string]: any }): { type: 'img'; props: any };
export declare function OffthreadVideo(props: MediaProps): { type: 'offthread-video'; props: MediaProps };
