//! Audio asset resolution and scene audio track inference.

use crate::composition::{CompositionError, PreparedComposition};
use rust_motion_project::Project;

/// Resolve audio assets declared by a project into encoder input paths.
///
/// Keeping this mapping in the shared host layer makes CLI and Tauri project
/// renders agree on which assets become audio streams.
pub fn project_audio_assets(project: &Project) -> Vec<std::path::PathBuf> {
    project
        .assets
        .iter()
        .filter(|asset| asset.kind == rust_motion_project::AssetKind::Audio)
        .map(|asset| std::path::PathBuf::from(&asset.path))
        .collect()
}

/// Resolve project audio assets relative to the project file directory.
/// URL assets remain unchanged for browser-side resolution.
pub fn project_audio_assets_from_dir(
    project: &Project,
    base_dir: impl AsRef<std::path::Path>,
) -> Vec<std::path::PathBuf> {
    let base_dir = base_dir.as_ref();
    project
        .assets
        .iter()
        .filter(|asset| asset.kind == rust_motion_project::AssetKind::Audio)
        .map(|asset| {
            if asset.path.contains("://") || asset.path.starts_with("data:") {
                std::path::PathBuf::from(&asset.path)
            } else {
                base_dir.join(&asset.path)
            }
        })
        .collect()
}

pub(crate) struct InferredAudioTrack {
    pub(crate) track_index: usize,
    pub(crate) track: rust_motion_rasterizer::AudioTrack,
    pub(crate) has_explicit_volume_keyframes: bool,
    pub(crate) last_sampled_volume: f64,
}

pub(crate) fn collect_scene_audio_tracks(
    prepared: &dyn PreparedComposition,
    first_scene: &rust_motion_rasterizer::Scene,
    duration_in_frames: u32,
    fps: f64,
) -> Result<Vec<rust_motion_rasterizer::AudioTrack>, CompositionError> {
    let mut tracks: Vec<InferredAudioTrack> = Vec::new();
    for frame in 0..duration_in_frames {
        let scene;
        let frame_tracks = if frame == 0 {
            first_scene.audio_tracks()
        } else {
            scene = prepared.render(frame)?;
            scene.audio_tracks()
        };
        for (index, mut candidate) in frame_tracks.into_iter().enumerate() {
            let sample_time = frame as f64 / fps;
            if candidate.timeline_start == 0.0 && frame > 0 {
                candidate.timeline_start = sample_time;
            }

            if let Some(existing) = tracks.iter_mut().find(|existing| {
                existing.track_index == index
                    && same_inferred_audio_track(&existing.track, &candidate)
            }) {
                match (existing.track.duration, candidate.duration) {
                    (None, Some(duration)) => existing.track.duration = Some(duration),
                    (Some(current), Some(candidate)) => {
                        existing.track.duration = Some(current.max(candidate));
                    }
                    _ => {}
                }

                if !candidate.volume_keyframes.is_empty() {
                    // An explicit curve is authoritative over inferred volume
                    // samples, including when it first appears on a later frame.
                    if !existing.has_explicit_volume_keyframes {
                        existing.track.volume = candidate.volume;
                        existing.track.volume_keyframes = candidate.volume_keyframes;
                        existing.has_explicit_volume_keyframes = true;
                    }
                } else if !existing.has_explicit_volume_keyframes
                    && candidate.volume != existing.last_sampled_volume
                {
                    if existing.track.volume_keyframes.is_empty() {
                        // Keyframe values are multipliers of the base volume in
                        // the FFmpeg path. Use a unity base and record absolute
                        // sampled gains so zero-to-positive changes are preserved.
                        existing
                            .track
                            .volume_keyframes
                            .push((existing.track.timeline_start, existing.track.volume));
                        existing.track.volume = 1.0;
                    }
                    existing
                        .track
                        .volume_keyframes
                        .push((sample_time, candidate.volume));
                }

                existing.last_sampled_volume = candidate.volume;
                continue;
            }

            tracks.push(InferredAudioTrack {
                track_index: index,
                has_explicit_volume_keyframes: !candidate.volume_keyframes.is_empty(),
                last_sampled_volume: candidate.volume,
                track: candidate,
            });
        }
    }
    Ok(tracks.into_iter().map(|inferred| inferred.track).collect())
}

pub(crate) fn same_inferred_audio_track(
    left: &rust_motion_rasterizer::AudioTrack,
    right: &rust_motion_rasterizer::AudioTrack,
) -> bool {
    left.src == right.src
        && left.start_from == right.start_from
        && left.playback_rate == right.playback_rate
        && left.looped == right.looped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_fixture() -> Project {
        Project {
            version: 1,
            composition: "test".into(),
            settings: rust_motion_project::ProjectSettings {
                width: 320,
                height: 240,
                fps: 30.0,
                duration: 30,
                scale: 1.0,
                crf: None,
                preset: None,
                concurrency: None,
                frame_step: 1,
                frame_start: None,
                frame_end: None,
                backend: rust_motion_project::BackendKind::Native,
                browser_image_format: None,
                browser_jpeg_quality: None,
                browser_frame_timeout_ms: None,
                browser_transport: None,
                browser_transport_retries: None,
            },
            props: serde_json::json!({}),
            assets: vec![],
            tracks: vec![],
            events: vec![],
            voice_over_asset_id: None,
        }
    }

    #[test]
    fn project_audio_assets_selects_only_audio_assets() {
        let project = Project {
            version: 1,
            composition: "test".into(),
            settings: rust_motion_project::ProjectSettings {
                width: 320,
                height: 240,
                fps: 30.0,
                duration: 30,
                scale: 1.0,
                crf: None,
                preset: None,
                concurrency: None,
                frame_step: 1,
                frame_start: None,
                frame_end: None,
                backend: rust_motion_project::BackendKind::Native,
                browser_image_format: None,
                browser_jpeg_quality: None,
                browser_frame_timeout_ms: None,
                browser_transport: None,
                browser_transport_retries: None,
            },
            props: serde_json::json!({}),
            assets: vec![
                rust_motion_project::AssetRef {
                    id: "music".into(),
                    path: "music.wav".into(),
                    kind: rust_motion_project::AssetKind::Audio,
                    sha256: None,
                },
                rust_motion_project::AssetRef {
                    id: "logo".into(),
                    path: "logo.png".into(),
                    kind: rust_motion_project::AssetKind::Image,
                    sha256: None,
                },
            ],
            tracks: vec![],
            events: vec![],
            voice_over_asset_id: None,
        };

        assert_eq!(
            project_audio_assets(&project),
            vec![std::path::PathBuf::from("music.wav")]
        );
    }

    #[test]
    fn project_audio_assets_from_dir_resolves_local_paths_without_touching_urls() {
        let project = Project {
            assets: vec![
                rust_motion_project::AssetRef {
                    id: "local".into(),
                    path: "audio/music.wav".into(),
                    kind: rust_motion_project::AssetKind::Audio,
                    sha256: None,
                },
                rust_motion_project::AssetRef {
                    id: "remote".into(),
                    path: "https://cdn.example/music.wav".into(),
                    kind: rust_motion_project::AssetKind::Audio,
                    sha256: None,
                },
            ],
            ..project_fixture()
        };
        assert_eq!(
            project_audio_assets_from_dir(&project, "/tmp/project"),
            vec![
                std::path::PathBuf::from("/tmp/project/audio/music.wav"),
                std::path::PathBuf::from("https://cdn.example/music.wav")
            ]
        );
    }
}
