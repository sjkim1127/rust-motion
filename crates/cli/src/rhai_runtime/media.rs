//! Scene builder methods for images, video, audio and Lottie/emoji assets.

use super::*;

impl SceneBuilder {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn image(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        fit: &str,
        opacity: FLOAT,
    ) -> RhaiResult<()> {
        if src.trim().is_empty() {
            return Err(runtime_error("image source path must not be empty".into()));
        }
        let opacity = finite_f32("opacity", opacity)?;
        if !(0.0..=1.0).contains(&opacity) {
            return Err(runtime_error(
                "opacity must be between 0.0 and 1.0".to_string(),
            ));
        }

        self.scene.push(SceneNode::Image {
            src: src.into_owned(),
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            fit: parse_image_fit(fit)?,
            opacity,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn video(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
        fit: &str,
        opacity: FLOAT,
    ) -> RhaiResult<()> {
        self.video_inner(x, y, w, h, src, time, fit, opacity, false)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn video_looped(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
        fit: &str,
        opacity: FLOAT,
        looped: bool,
    ) -> RhaiResult<()> {
        self.video_inner(x, y, w, h, src, time, fit, opacity, looped)
    }

    pub(super) fn emoji(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        size: FLOAT,
        emoji: ImmutableString,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::Emoji {
            emoji: emoji.into_owned(),
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            size: non_negative_f32("size", size)?,
            opacity: 1.0,
        });
        Ok(())
    }

    pub(super) fn lottie(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
    ) -> RhaiResult<()> {
        self.scene.push(SceneNode::Lottie {
            src: src.into_owned(),
            time: time.max(0.0),
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            playback_rate: 1.0,
            loop_behavior: rust_motion_rasterizer::LoopBehavior::Loop,
            opacity: 1.0,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn audio_visualizer(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
        color: ImmutableString,
        style_type: ImmutableString,
    ) -> RhaiResult<()> {
        let style = match style_type.to_lowercase().as_str() {
            "wave" => VisualizerStyle::Wave {
                stroke_width: 3.0,
                filled: true,
            },
            "radial" | "circle" => VisualizerStyle::Radial {
                radius: (h.min(w) * 0.35) as f32,
                bar_count: 48,
                bar_length: (h.min(w) * 0.25) as f32,
            },
            _ => VisualizerStyle::Bars {
                count: 32,
                gap: 4.0,
                radius: 2.0,
                mirror: false,
            },
        };

        self.scene.push(SceneNode::AudioVisualizer {
            src: src.into_owned(),
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            width: non_negative_f32("width", w)?,
            height: non_negative_f32("height", h)?,
            color: Color::from_hex(&color).unwrap_or(Color::WHITE),
            style,
            time: time.max(0.0),
            opacity: 1.0,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn video_inner(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        w: FLOAT,
        h: FLOAT,
        src: ImmutableString,
        time: FLOAT,
        fit: &str,
        opacity: FLOAT,
        looped: bool,
    ) -> RhaiResult<()> {
        validate_media_source(&src)?;
        let time = non_negative_f64("video time", time)?;
        let opacity = unit_f32("opacity", opacity)?;
        self.scene.push(SceneNode::Video {
            src: src.into_owned(),
            time,
            looped,
            x: finite_f32("x", x)?,
            y: finite_f32("y", y)?,
            w: non_negative_f32("width", w)?,
            h: non_negative_f32("height", h)?,
            fit: parse_image_fit(fit)?,
            opacity,
            interpolation: Default::default(),
            time_remap: None,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn audio(
        &mut self,
        src: ImmutableString,
        start_from: FLOAT,
        timeline_start: FLOAT,
        duration: FLOAT,
        volume: FLOAT,
        playback_rate: FLOAT,
        looped: bool,
    ) -> RhaiResult<()> {
        validate_media_source(&src)?;
        let duration = non_negative_f64("audio duration", duration)?;
        let playback_rate = finite_f64("audio playback rate", playback_rate)?;
        if !(0.5..=2.0).contains(&playback_rate) {
            return Err(runtime_error(
                "audio playback rate must be between 0.5 and 2.0".into(),
            ));
        }
        self.scene.push(SceneNode::Audio {
            track: AudioTrack {
                src: src.into_owned(),
                start_from: non_negative_f64("audio source offset", start_from)?,
                timeline_start: non_negative_f64("audio timeline offset", timeline_start)?,
                duration: (duration > 0.0).then_some(duration),
                volume: f64::from(unit_f32("audio volume", volume)?),
                playback_rate,
                looped,
                volume_keyframes: Vec::new(),
            },
        });
        Ok(())
    }

    pub(super) fn audio_simple(&mut self, src: ImmutableString, volume: FLOAT) -> RhaiResult<()> {
        self.audio(src, 0.0, 0.0, 0.0, volume, 1.0, false)
    }

    pub(super) fn audio_ducked(
        &mut self,
        src: ImmutableString,
        volume: FLOAT,
        keyframes: rhai::Array,
    ) -> RhaiResult<()> {
        validate_media_source(&src)?;
        let mut kfs = Vec::new();
        for item in keyframes {
            let arr = item.into_typed_array::<FLOAT>().map_err(|_| {
                runtime_error("audio keyframes must be [time, volume] arrays".into())
            })?;
            if arr.len() != 2 {
                return Err(runtime_error(
                    "audio keyframes must contain exactly [time, volume]".into(),
                ));
            }

            let time = non_negative_f64("audio keyframe time", arr[0])?;
            let volume = finite_f64("audio keyframe volume", arr[1])?;
            if !(0.0..=1.0).contains(&volume) {
                return Err(runtime_error(
                    "audio keyframe volume must be between 0.0 and 1.0".into(),
                ));
            }
            kfs.push((time, volume));
        }

        let mut track = AudioTrack::new(src.into_owned());
        track.volume = f64::from(unit_f32("audio volume", volume)?);
        track.volume_keyframes = kfs;
        self.scene.push(SceneNode::Audio { track });
        Ok(())
    }
}
