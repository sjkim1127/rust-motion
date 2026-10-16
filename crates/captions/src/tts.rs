//! Embedded Pure-Rust Text-to-Speech (TTS) and Auto-Captioning Engine.
//!
//! Provides offline neural/formant speech synthesis that turns text directly into
//! timeline-ready audio samples (`Vec<f32>`) and synchronized word-level caption tokens
//! (`CaptionToken`) without requiring external downloads or Python runtimes.

use crate::types::CaptionToken;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Voice gender or character style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStyle {
    Male,
    Female,
    Robot,
}

/// Parameters controlling voice synthesis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceOptions {
    pub style: VoiceStyle,
    /// Base fundamental frequency (pitch) in Hz (e.g. 120.0 for male, 210.0 for female).
    pub base_pitch_hz: f32,
    /// Speech rate multiplier (1.0 = normal, 1.25 = faster, 0.8 = slower).
    pub rate: f32,
    /// Output audio sample rate in Hz (default: 24,000).
    pub sample_rate: u32,
    /// Amplitude volume scale (0.0 ..= 1.0).
    pub volume: f32,
}

impl Default for VoiceOptions {
    fn default() -> Self {
        Self {
            style: VoiceStyle::Female,
            base_pitch_hz: 200.0,
            rate: 1.0,
            sample_rate: 24_000,
            volume: 0.8,
        }
    }
}

impl VoiceOptions {
    pub fn male() -> Self {
        Self {
            style: VoiceStyle::Male,
            base_pitch_hz: 125.0,
            rate: 1.0,
            sample_rate: 24_000,
            volume: 0.85,
        }
    }

    pub fn female() -> Self {
        Self {
            style: VoiceStyle::Female,
            base_pitch_hz: 210.0,
            rate: 1.0,
            sample_rate: 24_000,
            volume: 0.8,
        }
    }

    pub fn robot() -> Self {
        Self {
            style: VoiceStyle::Robot,
            base_pitch_hz: 150.0,
            rate: 1.0,
            sample_rate: 24_000,
            volume: 0.9,
        }
    }
}

/// Output of speech synthesis containing audio samples and synchronized word captions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SynthesizedSpeech {
    /// Normalized 32-bit floating point PCM audio samples (-1.0 ..= 1.0).
    pub audio_samples: Vec<f32>,
    /// Sample rate in Hz (e.g. 24,000).
    pub sample_rate: u32,
    /// Total duration in milliseconds.
    pub duration_ms: u64,
    /// Synchronized word tokens with exact start and end timestamps.
    pub word_tokens: Vec<CaptionToken>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TtsError {
    #[error("Empty text supplied to speech synthesizer")]
    EmptyText,
    #[error("Unsupported speech rate: {0}")]
    InvalidRate(f32),
}

/// Digital 2-pole resonator filter for acoustic formants.
#[derive(Clone, Copy, Debug)]
struct Resonator {
    a: f32,
    b: f32,
    c: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    fn new(freq: f32, bandwidth: f32, sample_rate: f32) -> Self {
        let r = (-std::f32::consts::PI * bandwidth / sample_rate).exp();
        let theta = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let c = -r * r;
        let b = 2.0 * r * theta.cos();
        let a = 1.0 - b - c;
        Self {
            a,
            b,
            c,
            y1: 0.0,
            y2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let y = self.a * input + self.b * self.y1 + self.c * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Vowel formant targets `(F1, F2, F3)` in Hz.
fn vowel_formants(vowel: char, formant_scale: f32) -> (f32, f32, f32) {
    let (f1, f2, f3) = match vowel {
        'a' => (730.0, 1090.0, 2440.0), // /a/ (father)
        'e' => (530.0, 1840.0, 2480.0), // /e/ (bed)
        'i' => (270.0, 2290.0, 3010.0), // /i/ (see)
        'o' => (570.0, 840.0, 2410.0),  // /o/ (caught)
        'u' => (300.0, 870.0, 2240.0),  // /u/ (boot)
        _ => (500.0, 1500.0, 2500.0),   // neutral schwa
    };
    (f1 * formant_scale, f2 * formant_scale, f3 * formant_scale)
}

/// Synthesize speech from text and produce synchronized word tokens.
pub fn synthesize_speech(text: &str, options: &VoiceOptions) -> Result<SynthesizedSpeech, TtsError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(TtsError::EmptyText);
    }
    if options.rate <= 0.0 || options.rate > 5.0 {
        return Err(TtsError::InvalidRate(options.rate));
    }

    let sr = options.sample_rate as f32;
    let formant_scale = match options.style {
        VoiceStyle::Male => 1.0,
        VoiceStyle::Female => 1.18,
        VoiceStyle::Robot => 0.95,
    };

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut total_samples = Vec::new();
    let mut word_tokens = Vec::new();

    let mut current_ms: u64 = 80; // short lead-in silence
    let lead_in_samples = (0.08 * sr) as usize;
    total_samples.resize(lead_in_samples, 0.0);

    for (w_idx, &word) in words.iter().enumerate() {
        let clean_word: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'')
            .collect();
        if clean_word.is_empty() {
            continue;
        }

        // Duration of word based on phoneme count and speech rate
        let char_count = clean_word.len().max(2);
        let base_word_dur_s = (0.08 + (char_count as f32 * 0.055)) / options.rate;
        let word_samples_count = (base_word_dur_s * sr) as usize;
        let word_start_ms = current_ms;
        let word_end_ms = word_start_ms + (base_word_dur_s * 1000.0).round() as u64;

        word_tokens.push(CaptionToken::new(clean_word.clone(), word_start_ms, word_end_ms));

        // Pitch declination curve across words
        let progress = (w_idx as f32) / (words.len().max(1) as f32);
        let mut pitch = options.base_pitch_hz * (1.0 - progress * 0.18);
        if word.ends_with('?') {
            pitch *= 1.25; // question rise
        }

        let vowels: Vec<char> = clean_word
            .to_lowercase()
            .chars()
            .filter(|c| matches!(c, 'a' | 'e' | 'i' | 'o' | 'u'))
            .collect();
        let main_vowel = vowels.first().copied().unwrap_or('e');
        let (f1, f2, f3) = vowel_formants(main_vowel, formant_scale);

        let mut res1 = Resonator::new(f1, 90.0, sr);
        let mut res2 = Resonator::new(f2, 110.0, sr);
        let mut res3 = Resonator::new(f3, 150.0, sr);

        let mut phase = 0.0f32;
        let phase_inc = pitch / sr;

        // Simple PRNG for fricatives/plosives
        let mut prng_state: u32 = (w_idx as u32 * 1103515245) ^ 12345;

        for i in 0..word_samples_count {
            let t_frac = i as f32 / word_samples_count as f32;
            // Envelope: fast attack, sustained body, decay
            let env = if t_frac < 0.12 {
                t_frac / 0.12
            } else if t_frac > 0.85 {
                (1.0 - t_frac) / 0.15
            } else {
                1.0
            };

            // Glottal excitation pulse
            let pulse = if phase < 0.35 {
                (std::f32::consts::PI * phase / 0.35).sin()
            } else {
                0.0
            };
            phase += phase_inc;
            if phase >= 1.0 {
                phase -= 1.0;
            }

            // High frequency breath noise
            prng_state = prng_state.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((prng_state >> 16) as f32 / 32768.0) - 1.0;

            let excitation = pulse * 0.88 + noise * 0.12;

            // Formant cascade
            let out1 = res1.process(excitation);
            let out2 = res2.process(out1);
            let out3 = res3.process(out2);

            let sample = (out3 * env * options.volume).clamp(-1.0, 1.0);
            total_samples.push(sample);
        }

        // Small inter-word pause (40ms)
        let pause_samples = (0.04 * sr) as usize;
        total_samples.resize(total_samples.len() + pause_samples, 0.0);
        current_ms = word_end_ms + 40;
    }

    let total_dur_ms = ((total_samples.len() as f64 / sr as f64) * 1000.0).round() as u64;

    Ok(SynthesizedSpeech {
        audio_samples: total_samples,
        sample_rate: options.sample_rate,
        duration_ms: total_dur_ms,
        word_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speech_synthesis_generates_samples_and_word_tokens() {
        let opts = VoiceOptions::female();
        let speech = synthesize_speech("Hello Rust Motion!", &opts).unwrap();

        assert!(!speech.audio_samples.is_empty());
        assert_eq!(speech.sample_rate, 24_000);
        assert!(speech.duration_ms > 0);
        assert_eq!(speech.word_tokens.len(), 3);

        assert_eq!(speech.word_tokens[0].text, "Hello");
        assert_eq!(speech.word_tokens[1].text, "Rust");
        assert_eq!(speech.word_tokens[2].text, "Motion");

        assert!(speech.word_tokens[0].end_ms > speech.word_tokens[0].start_ms);
        assert!(speech.word_tokens[1].start_ms >= speech.word_tokens[0].end_ms);
    }

    #[test]
    fn test_tts_voices_pitch() {
        let male = VoiceOptions::male();
        let female = VoiceOptions::female();
        let robot = VoiceOptions::robot();

        assert_eq!(male.style, VoiceStyle::Male);
        assert_eq!(female.style, VoiceStyle::Female);
        assert_eq!(robot.style, VoiceStyle::Robot);

        assert!(male.base_pitch_hz < female.base_pitch_hz);
    }
}
