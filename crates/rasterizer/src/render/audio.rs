//! Audio track validation and the FFmpeg audio filter graph (volume envelopes, trimming, mixing).

use super::*;

pub(super) fn validate_audio_tracks(
    tracks: &[AudioTrack],
    policy: &MediaSecurityPolicy,
) -> Result<(), RasterError> {
    for track in tracks {
        policy.validate_path(&track.src)?;
        if !track.start_from.is_finite() || track.start_from < 0.0 {
            return Err(invalid_audio(
                track,
                "start_from must be finite and non-negative",
            ));
        }
        if !track.timeline_start.is_finite() || track.timeline_start < 0.0 {
            return Err(invalid_audio(
                track,
                "timeline_start must be finite and non-negative",
            ));
        }
        if track
            .duration
            .is_some_and(|duration| !duration.is_finite() || duration <= 0.0)
        {
            return Err(invalid_audio(track, "duration must be finite and positive"));
        }
        if !track.volume.is_finite() || !(0.0..=1.0).contains(&track.volume) {
            return Err(invalid_audio(track, "volume must be between 0.0 and 1.0"));
        }
        if !track.playback_rate.is_finite() || !(0.5..=2.0).contains(&track.playback_rate) {
            return Err(invalid_audio(
                track,
                "playback_rate must be between 0.5 and 2.0",
            ));
        }
        for (time, volume) in &track.volume_keyframes {
            if !time.is_finite() || *time < 0.0 {
                return Err(invalid_audio(
                    track,
                    "volume keyframe time must be finite and non-negative",
                ));
            }
            if !volume.is_finite() || !(0.0..=1.0).contains(volume) {
                return Err(invalid_audio(
                    track,
                    "volume keyframe gain must be between 0.0 and 1.0",
                ));
            }
        }
    }
    Ok(())
}

fn invalid_audio(track: &AudioTrack, reason: &str) -> RasterError {
    RasterError::MediaAsset {
        path: track.src.clone(),
        reason: reason.into(),
    }
}

pub(super) fn active_audio_tracks(config: &PipeConfig) -> Vec<&AudioTrack> {
    let range_start = config.start_frame as f64 / config.fps;
    let output_fps = config.fps / config.frame_step as f64;
    let range_end = range_start + config.duration_in_frames as f64 / output_fps;
    config
        .audio_tracks
        .iter()
        .filter(|track| {
            let track_end = track
                .duration
                .map(|duration| track.timeline_start + duration)
                .unwrap_or(f64::INFINITY);
            track.timeline_start < range_end && track_end > range_start
        })
        .collect()
}

fn build_volume_expression(base_volume: f64, keyframes: &[(f64, f64)]) -> String {
    if keyframes.is_empty() {
        return format!("{:.6}", base_volume);
    }
    if keyframes.len() == 1 {
        return format!("{:.6}", keyframes[0].1 * base_volume);
    }

    let mut sorted = keyframes.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let last_val = sorted.last().unwrap().1 * base_volume;
    let mut expr = format!("{:.6}", last_val);

    for i in (0..sorted.len() - 1).rev() {
        let (t0, v0_raw) = sorted[i];
        let (t1, v1_raw) = sorted[i + 1];
        let v0 = v0_raw * base_volume;
        let v1 = v1_raw * base_volume;
        let dt = (t1 - t0).max(0.0001);

        let interp = format!("({:.6}+({:.6})*(t-({:.6}))/{:.6})", v0, v1 - v0, t0, dt);
        if i == 0 {
            expr = format!(
                "if(lt(t,{:.6}),{:.6},if(lt(t,{:.6}),{},{}))",
                t0, v0, t1, interp, expr
            );
        } else {
            expr = format!("if(lt(t,{:.6}),{},{})", t1, interp, expr);
        }
    }

    expr
}

pub(super) fn build_audio_filter(config: &PipeConfig, tracks: &[&AudioTrack]) -> String {
    let mut filters = Vec::with_capacity(tracks.len() + 1);
    let range_start = config.start_frame as f64 / config.fps;
    for (index, track) in tracks.iter().enumerate() {
        let skipped_timeline = (range_start - track.timeline_start).max(0.0);
        let source_start = track.start_from + skipped_timeline * track.playback_rate;
        let mut chain = format!(
            "[{}:a:0]atrim=start={:.9},asetpts=PTS-STARTPTS,atempo={:.9}",
            index + 1,
            source_start,
            track.playback_rate,
        );
        if track.volume_keyframes.is_empty() {
            chain.push_str(&format!(",volume={:.9}", track.volume));
        } else {
            // `asetpts=PTS-STARTPTS` resets the trimmed audio stream to t=0.
            // Keyframes use absolute composition seconds, so shift them to the
            // first composition time represented by this trimmed stream.
            let keyframe_origin = range_start.max(track.timeline_start);
            let local_keyframes = track
                .volume_keyframes
                .iter()
                .map(|(time, volume)| (time - keyframe_origin, *volume))
                .collect::<Vec<_>>();
            let expr = build_volume_expression(track.volume, &local_keyframes);
            chain.push_str(&format!(",volume='{}':eval=frame", expr));
        }
        if let Some(duration) = track.duration {
            let remaining = (duration - skipped_timeline).max(0.0);
            chain.push_str(&format!(",atrim=duration={remaining:.9}"));
        }
        let relative_start = (track.timeline_start - range_start).max(0.0);
        if relative_start > 0.0 {
            let delay_ms = (relative_start * 1000.0).round() as u64;
            chain.push_str(&format!(",adelay={delay_ms}:all=1"));
        }
        chain.push_str(&format!("[a{index}]"));
        filters.push(chain);
    }

    let labels = (0..tracks.len())
        .map(|index| format!("[a{index}]"))
        .collect::<String>();
    let output_fps = config.fps / config.frame_step as f64;
    let output_duration = config.duration_in_frames as f64 / output_fps;
    if tracks.len() == 1 {
        filters.push(format!(
            "{labels}apad,atrim=duration={output_duration:.9},asetpts=N/SR/TB[aout]"
        ));
    } else {
        filters.push(format!(
            "{labels}amix=inputs={}:normalize=0:duration=longest,apad,atrim=duration={output_duration:.9},asetpts=N/SR/TB[aout]",
            tracks.len()
        ));
    }
    filters.join(";")
}

pub(super) const NATIVE_AUDIO_SAMPLE_RATE: u32 = 48_000;
const MAX_MIX_DURATION_SECS: f64 = 7200.0;

fn evaluate_volume_keyframes(base_volume: f64, keyframes: &[(f64, f64)], t: f64) -> f64 {
    if keyframes.is_empty() {
        return base_volume;
    }
    if keyframes.len() == 1 {
        return (keyframes[0].1 * base_volume).clamp(0.0, 1.0);
    }
    let mut sorted = keyframes.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    if t <= sorted[0].0 {
        return (sorted[0].1 * base_volume).clamp(0.0, 1.0);
    }
    for win in sorted.windows(2) {
        let (t0, v0) = win[0];
        let (t1, v1) = win[1];
        if t <= t1 {
            let dt = (t1 - t0).max(0.0001);
            let frac = ((t - t0) / dt).clamp(0.0, 1.0);
            return ((v0 + (v1 - v0) * frac) * base_volume).clamp(0.0, 1.0);
        }
    }
    (sorted[sorted.len() - 1].1 * base_volume).clamp(0.0, 1.0)
}

/// Decode, trim, rate-stretch, envelope, loop, and mix all active audio tracks into
/// interleaved stereo `f32` PCM (`[L0, R0, L1, R1, ...]`) at `target_sample_rate`.
pub(super) fn mix_audio_tracks_pcm(
    config: &PipeConfig,
    target_sample_rate: u32,
) -> Result<Vec<f32>, RasterError> {
    if target_sample_rate == 0 {
        return Err(RasterError::ImageEncode(
            "audio target sample rate must be greater than zero".into(),
        ));
    }
    let output_fps = config.fps / f64::from(config.frame_step.max(1));
    let output_duration = f64::from(config.duration_in_frames) / output_fps;
    if !output_duration.is_finite()
        || output_duration <= 0.0
        || output_duration > MAX_MIX_DURATION_SECS
    {
        return Err(RasterError::ImageEncode(format!(
            "output audio duration ({output_duration:.2}s) exceeds native mix bounds"
        )));
    }

    let sr_f64 = f64::from(target_sample_rate);
    let total_out_frames = (output_duration * sr_f64).ceil().max(1.0) as usize;
    let mut mix = vec![0.0f32; total_out_frames * 2];

    let tracks = active_audio_tracks(config);
    if tracks.is_empty() {
        return Ok(mix);
    }

    let range_start = f64::from(config.start_frame) / config.fps;

    for track in tracks {
        let local_path = config
            .security_policy
            .validate_path(&track.src)
            .map_err(|e| {
                RasterError::ImageEncode(format!("Audio path rejected ({}): {e}", track.src))
            })?;
        let audio = crate::audio_cache::decode_audio_file(&local_path).map_err(|e| {
            RasterError::ImageEncode(format!("Failed to decode audio {}: {e}", track.src))
        })?;
        if audio.channel_waveforms.is_empty() || audio.sample_rate == 0 {
            continue;
        }
        let src_frames = audio.channel_waveforms[0].len();
        if src_frames == 0 {
            continue;
        }
        let left = &audio.channel_waveforms[0];
        let right = audio
            .channel_waveforms
            .get(1)
            .unwrap_or(&audio.channel_waveforms[0]);

        let mut sorted_kf = track.volume_keyframes.clone();
        sorted_kf.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let skipped_timeline = (range_start - track.timeline_start).max(0.0);
        let source_start = track.start_from + skipped_timeline * track.playback_rate;
        let relative_start = (track.timeline_start - range_start).max(0.0);
        let track_end_local = track
            .duration
            .map(|dur| relative_start + (dur - skipped_timeline).max(0.0))
            .unwrap_or(output_duration)
            .min(output_duration);

        if track_end_local <= relative_start {
            continue;
        }

        let start_out = (relative_start * sr_f64).floor().max(0.0) as usize;
        let end_out = ((track_end_local * sr_f64).ceil() as usize).min(total_out_frames);
        let src_sr = f64::from(audio.sample_rate);
        let src_frames_f64 = src_frames as f64;

        for out_idx in start_out..end_out {
            let out_t = out_idx as f64 / sr_f64;
            let local_t = (out_t - relative_start).max(0.0);
            let src_pos = (source_start + local_t * track.playback_rate) * src_sr;
            if src_pos < 0.0 {
                continue;
            }
            let (i0, i1, frac) = if track.looped {
                let wrapped = src_pos.rem_euclid(src_frames_f64);
                let i0 = (wrapped.floor() as usize).min(src_frames - 1);
                let i1 = (i0 + 1) % src_frames;
                (i0, i1, (wrapped - i0 as f64) as f32)
            } else {
                if src_pos >= src_frames_f64 {
                    break;
                }
                let i0 = (src_pos.floor() as usize).min(src_frames - 1);
                let i1 = (i0 + 1).min(src_frames - 1);
                (i0, i1, (src_pos - i0 as f64) as f32)
            };

            let comp_t = range_start + out_t;
            let gain = evaluate_volume_keyframes(track.volume, &sorted_kf, comp_t) as f32;
            let s_l = left[i0] + (left[i1] - left[i0]) * frac;
            let s_r = right[i0] + (right[i1] - right[i0]) * frac;
            mix[out_idx * 2] += s_l * gain;
            mix[out_idx * 2 + 1] += s_r * gain;
        }
    }

    for sample in &mut mix {
        *sample = sample.clamp(-1.0, 1.0);
    }

    Ok(mix)
}
