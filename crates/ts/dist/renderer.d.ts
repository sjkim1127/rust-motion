import type { CompositionProps, VideoConfig } from './index.d.ts';

export interface RenderProgress {
  renderedFrames: number;
  encodedFrames: number;
  totalFrames: number;
  progress: number;
}

export interface RenderMediaOptions {
  composition: string | CompositionProps;
  outputLocation: string;
  serveUrl?: string;
  entryPoint?: string;
  codec?: 'h264' | 'h265' | 'vp9' | 'av1' | 'prores' | 'gif' | 'png' | 'webm';
  crf?: number;
  hwAccel?: 'auto' | 'disabled' | 'videotoolbox' | 'nvenc';
  concurrency?: number;
  inputProps?: Record<string, unknown>;
  onProgress?: (progress: RenderProgress) => void;
}

export interface RenderMediaResult {
  outputLocation: string;
  totalFrames: number;
  width: number;
  height: number;
  fps: number;
}

export declare function renderMedia(options: RenderMediaOptions): Promise<RenderMediaResult>;

export interface RenderStillOptions {
  composition: string | CompositionProps;
  output: string;
  frame?: number;
  imageFormat?: 'png' | 'jpeg' | 'webp';
  inputProps?: Record<string, unknown>;
}

export interface RenderStillResult {
  output: string;
  frame: number;
  width: number;
  height: number;
  imageFormat: string;
}

export declare function renderStill(options: RenderStillOptions): Promise<RenderStillResult>;

export declare function getCompositions(entryPoint?: string): Promise<CompositionProps[]>;
export declare function selectComposition(options: { id: string; entryPoint?: string }): Promise<CompositionProps>;
