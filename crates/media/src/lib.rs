//! # rust-motion-media
//!
//! Native media helpers and scene emitters, with optional Dioxus media components.
//!
//! Disable the default Dioxus feature when only native scene and asset APIs are needed.

#[cfg(feature = "dioxus")]
pub mod audio;
pub mod audio_viz;
#[cfg(feature = "dioxus")]
pub mod canvas_image;
pub mod ducking;
#[cfg(feature = "dioxus")]
pub mod gif;
pub mod img;
pub mod metadata;
pub mod sample;
pub mod scene;
#[cfg(feature = "dioxus")]
pub mod video;

#[cfg(feature = "dioxus")]
pub use audio::{Audio, AudioProps, Html5Audio};
pub use audio_viz::{
    create_smooth_svg_path, get_waveform_bars, get_waveform_portion, get_waveform_portion_channel,
    load_audio_data, visualize_audio, visualize_audio_waveform, AudioData, AudioVizError,
    VisualizeAudioWaveformOptions, VisualizeFor, WaveformBarsOptions,
};
#[cfg(feature = "dioxus")]
pub use canvas_image::{CanvasImage, CanvasImageProps};
pub use ducking::{
    calculate_ducking_envelope, merge_speech_intervals, DuckingOptions, SpeechInterval,
};
#[cfg(feature = "dioxus")]
pub use gif::{Gif, GifProps};
pub use img::ImageFit;
#[cfg(feature = "dioxus")]
pub use img::{Img, ImgProps};
pub use metadata::{
    get_audio_metadata, get_image_dimensions, get_video_metadata, parse_media, read_media_range,
    static_file, AudioMetadata, ImageDimensions, MediaMetadataError, ParsedMediaMetadata,
    VideoMetadata, MAX_MEDIA_RANGE_BYTES,
};
pub use sample::{read_encoded_sample, read_encoded_samples, EncodedSample};
pub use scene::{SceneAudio, SceneGif, SceneImage, SceneVideo};
pub use rust_motion_rasterizer::{
    FrameInterpolationMode, OpticalFlowEngine, TimeRemapCurve, TimeRemapPoint,
};
pub use rust_motion_audio_dsp as audio_dsp;
#[cfg(feature = "dioxus")]
pub use video::{Html5Video, OffthreadVideo, Video, VideoProps};
