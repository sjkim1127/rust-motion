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
