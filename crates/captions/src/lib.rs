//! Rust Motion Captions — Subtitle parsing, word timing, and kinetic animated subtitles.
//!
//! Ported and extended from `@remotion/captions`:
//! - [`parse_srt`] / [`serialize_srt`]
//! - [`parse_whisper_json`] (OpenAI / Faster-Whisper word timestamps)
//! - [`parse_vtt`] (WebVTT format)
//! - [`wrap_caption_tokens_to_lines`] (pixel-width multi-line wrapping)
//! - [`ensure_max_characters_per_line`]
//! - [`create_tiktok_style_captions`]
//! - [`TikTokCaptions`] & [`SceneCaptions`]

pub mod ass;
pub mod layout;
pub mod line_wrapper;
pub mod scene;
pub mod srt_parser;
pub mod tiktok_captions;
pub mod tts;
pub mod types;
pub mod whisper_parser;

pub use ass::{format_ass_timestamp, parse_ass, parse_ass_timestamp, serialize_ass_karaoke, AssDialogue, AssError};
pub use layout::{wrap_caption_tokens_to_lines, CaptionLineLayout};
pub use line_wrapper::ensure_max_characters_per_line;
pub use scene::SceneCaptions;
pub use srt_parser::{format_srt_timestamp, parse_srt, serialize_srt, CaptionParseError};
pub use tiktok_captions::create_tiktok_style_captions;
#[cfg(feature = "dioxus")]
pub use tiktok_captions::{TikTokCaptions, TikTokCaptionsProps};
pub use tts::{synthesize_speech, SynthesizedSpeech, TtsError, VoiceOptions, VoiceStyle};
pub use types::{CaptionPage, CaptionToken};
pub use whisper_parser::{parse_vtt, parse_whisper_json, SubtitleError};
