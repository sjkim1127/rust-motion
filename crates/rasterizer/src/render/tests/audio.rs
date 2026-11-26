//! Audio filter graph construction and track validation.

use super::*;

#[test]
fn test_audio_filter_args_mix_tracks() {
    let mut first = AudioTrack::new("first.wav");
    first.start_from = 0.25;
    first.timeline_start = 0.5;
    first.volume = 0.75;
    let mut second = AudioTrack::new("second.wav");
    second.looped = true;
    second.playback_rate = 1.25;
    let config =
        PipeConfig::new(1920, 1080, 30.0, 90, "/tmp/out.mp4").with_audio_tracks([first, second]);

    let args = build_pipe_ffmpeg_args(&config);
    let filter_index = args
        .iter()
        .position(|arg| arg == "-filter_complex")
        .unwrap();
    let filter = &args[filter_index + 1];
    assert!(filter.contains("adelay=500:all=1"));
    assert!(filter.contains("atempo=1.250000000"));
    assert!(filter.contains("amix=inputs=2"));
    assert!(args.contains(&"-stream_loop".to_string()));
    assert!(args.contains(&"[aout]".to_string()));
}

#[test]
fn test_audio_filter_trims_against_selected_frame_range() {
    let mut track = AudioTrack::new("track.wav");
    track.start_from = 0.25;
    track.duration = Some(5.0);
    let config = PipeConfig::new(64, 64, 30.0, 30, "out.mp4")
        .with_frame_start(60)
        .with_audio_tracks([track]);
    let args = build_pipe_ffmpeg_args(&config);
    let filter_index = args
        .iter()
        .position(|arg| arg == "-filter_complex")
        .unwrap();
    let filter = &args[filter_index + 1];
    assert!(filter.contains("atrim=start=2.250000000"));
    assert!(filter.contains("atrim=duration=3.000000000"));
    assert!(filter.contains("atrim=duration=1.000000000"));
    assert!(!filter.contains("adelay="));
}

#[test]
fn test_audio_volume_keyframes_use_trimmed_composition_timeline() {
    let mut track = AudioTrack::new("track.wav");
    track.timeline_start = 5.0;
    track.volume = 0.5;
    track.volume_keyframes = vec![(5.0, 0.0), (7.0, 1.0), (9.0, 0.4)];
    let config = PipeConfig::new(64, 64, 30.0, 300, "out.mp4").with_audio_tracks([track]);
    let args = build_pipe_ffmpeg_args(&config);
    let filter_index = args
        .iter()
        .position(|arg| arg == "-filter_complex")
        .unwrap();
    let filter = &args[filter_index + 1];

    assert!(filter.contains("volume='if(lt(t,0.000000),0.000000,if(lt(t,2.000000)"));
    assert!(filter.contains("(t-(0.000000))/2.000000"));
    assert!(filter.contains("lt(t,4.000000)"));
    assert!(filter.contains("adelay=5000:all=1"));
}

#[test]
fn test_partial_audio_range_rebases_keyframes_and_keeps_prior_envelope() {
    let mut track = AudioTrack::new("track.wav");
    track.timeline_start = 5.0;
    track.volume_keyframes = vec![(5.0, 0.0), (7.0, 1.0), (9.0, 0.4)];
    let config = PipeConfig::new(64, 64, 30.0, 90, "out.mp4")
        .with_frame_start(180)
        .with_audio_tracks([track]);
    let args = build_pipe_ffmpeg_args(&config);
    let filter_index = args
        .iter()
        .position(|arg| arg == "-filter_complex")
        .unwrap();
    let filter = &args[filter_index + 1];

    assert!(filter.contains("atrim=start=1.000000000"));
    assert!(filter.contains("volume='if(lt(t,-1.000000),0.000000,if(lt(t,1.000000)"));
    assert!(filter.contains("(t-(-1.000000))/2.000000"));
    assert!(filter.contains("lt(t,3.000000)"));
    assert!(!filter.contains("adelay="));
}

#[test]
fn test_audio_track_validation_rejects_invalid_volume() {
    let mut track = AudioTrack::new(std::env::current_exe().unwrap().display().to_string());
    track.volume = 1.5;
    let error = validate_audio_tracks(&[track], &MediaSecurityPolicy::default()).unwrap_err();
    assert!(error.to_string().contains("volume must be between"));
}

#[test]
fn test_audio_track_validation_rejects_invalid_volume_keyframes() {
    let src = std::env::current_exe().unwrap().display().to_string();
    let mut negative_time = AudioTrack::new(&src);
    negative_time.volume_keyframes = vec![(-0.1, 0.5)];
    let error =
        validate_audio_tracks(&[negative_time], &MediaSecurityPolicy::default()).unwrap_err();
    assert!(error.to_string().contains("keyframe time"));

    let mut invalid_gain = AudioTrack::new(src);
    invalid_gain.volume_keyframes = vec![(0.0, 1.1)];
    let error =
        validate_audio_tracks(&[invalid_gain], &MediaSecurityPolicy::default()).unwrap_err();
    assert!(error.to_string().contains("keyframe gain"));
}
