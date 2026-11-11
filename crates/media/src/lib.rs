//! # rust-motion-media
//!
//! Native media decoders, metadata extractors, and audio visualization for Rust Motion.

pub mod audio_viz;
pub mod ducking;
pub mod img;
pub mod metadata;
pub mod sample;
pub mod scene;

pub use audio_viz::{
    create_smooth_svg_path, get_waveform_bars, get_waveform_portion, get_waveform_portion_channel,
    load_audio_data, visualize_audio, visualize_audio_waveform, AudioData, AudioVizError,
    VisualizeAudioWaveformOptions, VisualizeFor, WaveformBarsOptions,
};
pub use ducking::{
    calculate_ducking_envelope, merge_speech_intervals, DuckingOptions, SpeechInterval,
};
pub use img::ImageFit;
pub use metadata::{
    get_audio_metadata, get_image_dimensions, get_video_metadata, parse_media, read_media_range,
    static_file, AudioMetadata, ImageDimensions, MediaMetadataError, ParsedMediaMetadata,
    VideoMetadata, MAX_MEDIA_RANGE_BYTES,
};
pub use rust_motion_audio_dsp as audio_dsp;
pub use rust_motion_rasterizer::{
    FrameInterpolationMode, OpticalFlowEngine, TimeRemapCurve, TimeRemapPoint,
};
pub use sample::{read_encoded_sample, read_encoded_samples, EncodedSample};
pub use scene::{SceneAudio, SceneGif, SceneImage, SceneVideo};
