export interface PlayerProps<T = any> {
  component: (props: T) => any;
  durationInFrames: number;
  compositionWidth: number;
  compositionHeight: number;
  fps: number;
  initialFrame?: number;
  inputProps?: T;
  autoPlay?: boolean;
  loop?: boolean;
  controls?: boolean;
  style?: Record<string, any>;
}

export declare function Player<T = any>(props: PlayerProps<T>): any;

export interface ThumbnailProps<T = any> {
  component: (props: T) => any;
  frame?: number;
  compositionWidth?: number;
  compositionHeight?: number;
  fps?: number;
  inputProps?: T;
}

export declare function Thumbnail<T = any>(props: ThumbnailProps<T>): any;
