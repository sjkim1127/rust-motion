//! Hardened Rhai engine construction and the registered scene API surface.

use super::*;

pub(super) fn hardened_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_module_resolver(DummyModuleResolver::new());
    engine.set_max_operations(MAX_OPERATIONS_PER_FRAME);
    engine.set_max_call_levels(32);
    engine.set_max_expr_depths(64, 32);
    engine.set_max_variables(256);
    engine.set_max_functions(128);
    engine.set_max_string_size(MAX_STRING_SIZE);
    engine.set_max_array_size(MAX_ARRAY_SIZE);
    engine.set_max_map_size(MAX_MAP_SIZE);
    engine
}

pub(super) fn register_scene_api(engine: &mut Engine) {
    engine.register_type_with_name::<SceneBuilder>("Scene");
    engine.register_fn("scene", SceneBuilder::new);
    engine.register_fn("rect", SceneBuilder::rect);
    engine.register_fn("round_rect", SceneBuilder::round_rect);
    engine.register_fn("rect_stroke", SceneBuilder::rect_stroke);
    engine.register_fn("circle", SceneBuilder::circle);
    engine.register_fn("circle_stroke", SceneBuilder::circle_stroke);
    engine.register_fn("linear_gradient", SceneBuilder::linear_gradient);
    engine.register_fn("path", SceneBuilder::path);
    engine.register_fn("text", SceneBuilder::text);
    engine.register_fn("text_bold", SceneBuilder::text_bold);
    engine.register_fn("text_font", SceneBuilder::text_font);
    engine.register_fn("text_box", SceneBuilder::text_box);
    engine.register_fn("image", SceneBuilder::image);
    engine.register_fn("video", SceneBuilder::video);
    engine.register_fn("video", SceneBuilder::video_looped);
    engine.register_fn("audio", SceneBuilder::audio);
    engine.register_fn("audio", SceneBuilder::audio_simple);
    engine.register_fn("audio_ducked", SceneBuilder::audio_ducked);
    engine.register_fn("emoji", SceneBuilder::emoji);
    engine.register_fn("lottie", SceneBuilder::lottie);
    engine.register_fn("audio_visualizer", SceneBuilder::audio_visualizer);
    engine.register_fn("group", SceneBuilder::group);
    engine.register_fn("flex_row", SceneBuilder::flex_row);
    engine.register_fn("flex_col", SceneBuilder::flex_col);
    engine.register_fn("grid", SceneBuilder::grid);
    engine.register_fn("svg", SceneBuilder::svg);
    engine.register_fn("svg_at", SceneBuilder::svg_at);
    engine.register_fn("mermaid", SceneBuilder::mermaid);
    engine.register_fn("mermaid", SceneBuilder::mermaid_opts);
    engine.register_fn("d3_line", SceneBuilder::d3_line);
    engine.register_fn("d3_bar", SceneBuilder::d3_bar);
    engine.register_fn("d3_pie", SceneBuilder::d3_pie);
    engine.register_fn("d3_eval", SceneBuilder::d3_eval);
    engine.register_fn("confetti", SceneBuilder::confetti);
    engine.register_fn("confetti", SceneBuilder::confetti_at);
    engine.register_fn("confetti_cannon", SceneBuilder::confetti_cannon);
    engine.register_fn("mesh_3d", SceneBuilder::mesh_3d);
    engine.register_fn("gltf", SceneBuilder::gltf);
    engine.register_fn("gltf", |scene: &mut SceneBuilder, source: &str| {
        scene.gltf(source, Map::new())
    });
    engine.register_fn("shader", SceneBuilder::shader);
    engine.register_fn("shader", |scene: &mut SceneBuilder, source: &str| {
        scene.shader(source, Map::new())
    });

    engine.register_type_with_name::<RhaiLayout>("RhaiLayout");
    engine.register_fn("rect", RhaiLayout::rect);
    engine.register_fn("round_rect", RhaiLayout::round_rect);
    engine.register_fn("circle", RhaiLayout::circle);
    engine.register_fn("text", RhaiLayout::text);
    engine.register_fn("text_bold", RhaiLayout::text_bold);
    engine.register_fn("add", SceneBuilder::add);
    engine.register_fn("add_layout", SceneBuilder::add_layout);
    engine.register_fn(
        "interpolate",
        |value: FLOAT,
         input_start: FLOAT,
         input_end: FLOAT,
         output_start: FLOAT,
         output_end: FLOAT| {
            if input_start == input_end {
                return output_end;
            }
            let t = ((value - input_start) / (input_end - input_start)).clamp(0.0, 1.0);
            output_start + (output_end - output_start) * t
        },
    );
    engine.register_fn(
        "interpolate_colors",
        |from: ImmutableString, to: ImmutableString, t: FLOAT| -> ImmutableString {
            rust_motion_animation::interpolate_colors(from.as_str(), to.as_str(), t).into()
        },
    );
    engine.register_fn("random", |seed: ImmutableString| -> FLOAT {
        rust_motion_animation::random(seed.as_str())
    });
    engine.register_fn("random", |seed: INT| -> FLOAT {
        rust_motion_animation::random(seed as f64)
    });
    engine.register_fn("random", |seed: FLOAT| -> FLOAT {
        rust_motion_animation::random(seed)
    });
    engine.register_fn("spring", |frame: FLOAT, fps: FLOAT| -> FLOAT {
        let config = rust_motion_animation::SpringConfig::default();
        rust_motion_animation::spring_with_options(
            frame,
            fps,
            config,
            rust_motion_animation::SpringOptions::default(),
        )
        .unwrap_or(1.0)
    });
    engine.register_fn(
        "spring",
        |frame: FLOAT, fps: FLOAT, damping: FLOAT, mass: FLOAT, stiffness: FLOAT| -> FLOAT {
            let config = rust_motion_animation::SpringConfig {
                damping,
                mass,
                stiffness,
                overshoot_clamping: false,
            };
            rust_motion_animation::spring_with_options(
                frame,
                fps,
                config,
                rust_motion_animation::SpringOptions::default(),
            )
            .unwrap_or(1.0)
        },
    );
    engine.register_fn("static_file", |path: ImmutableString| -> ImmutableString {
        rust_motion_media::static_file(path.as_str())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string())
            .into()
    });
    engine.register_fn("cube_3d", SceneBuilder::cube_3d);
    engine.register_fn("sphere_3d", SceneBuilder::sphere_3d);
    engine.register_fn("torus_3d", SceneBuilder::torus_3d);
    engine.register_fn(
        "lfo",
        |frame: FLOAT, fps: FLOAT, freq_hz: FLOAT, amp: FLOAT| -> FLOAT {
            let lfo =
                rust_motion_animation::LfoChop::new(rust_motion_animation::LfoWave::Sine, freq_hz)
                    .with_amplitude(amp);
            lfo.sample(frame, fps)
        },
    );
    engine.register_fn(
        "lag",
        |current: FLOAT, target: FLOAT, dt: FLOAT, lag_sec: FLOAT| -> FLOAT {
            let filter = rust_motion_animation::LagChop::new(lag_sec, lag_sec);
            filter.filter(current, target, dt)
        },
    );
    engine.register_fn("clamp", |val: FLOAT, min: FLOAT, max: FLOAT| -> FLOAT {
        if val.is_nan() {
            min
        } else {
            val.clamp(min, max)
        }
    });
    engine.register_fn(
        "typewriter_text",
        |text: ImmutableString, progress: FLOAT| -> ImmutableString {
            rust_motion_animation::typewriter_text(&text, progress, true, '|').into()
        },
    );
    engine.register_fn(
        "scramble_text",
        |text: ImmutableString, progress: FLOAT, seed: INT| -> ImmutableString {
            rust_motion_animation::scramble_text(&text, progress, seed as u64).into()
        },
    );
    engine.register_fn("make_ellipse", |rx: FLOAT, ry: FLOAT| -> ImmutableString {
        rust_motion_shapes::make_ellipse(rx, ry).path.into()
    });
    engine.register_fn(
        "safe_area_insets",
        |platform: ImmutableString, w: FLOAT, h: FLOAT| -> Map {
            let plat = match platform.to_lowercase().as_str() {
                "reels" | "instagram" => rust_motion_composition::Platform::InstagramReels,
                "shorts" | "youtube" => rust_motion_composition::Platform::YouTubeShorts,
                "action" => rust_motion_composition::Platform::ActionSafe,
                "title" => rust_motion_composition::Platform::TitleSafe,
                _ => rust_motion_composition::Platform::TikTok,
            };
            let insets = rust_motion_composition::get_safe_area_insets(plat, w as f32, h as f32);
            let mut map = Map::new();
            map.insert("top".into(), Dynamic::from(insets.top as FLOAT));
            map.insert("bottom".into(), Dynamic::from(insets.bottom as FLOAT));
            map.insert("left".into(), Dynamic::from(insets.left as FLOAT));
            map.insert("right".into(), Dynamic::from(insets.right as FLOAT));
            map
        },
    );
    engine.register_fn(
        "fit_text",
        |text: ImmutableString, max_w: FLOAT, max_h: FLOAT| -> FLOAT {
            rust_motion_composition::fit_text(&text, max_w as f32, max_h as f32, 10.0, 120.0)
                as FLOAT
        },
    );
    engine.register_fn("path_bounding_box", |path: ImmutableString| -> Map {
        let mut map = Map::new();
        if let Some(bbox) = rust_motion_paths::get_bounding_box(&path) {
            map.insert("x".into(), Dynamic::from(bbox.x));
            map.insert("y".into(), Dynamic::from(bbox.y));
            map.insert("width".into(), Dynamic::from(bbox.width));
            map.insert("height".into(), Dynamic::from(bbox.height));
        }
        map
    });
    engine.register_fn(
        "rotate_path",
        |path: ImmutableString, angle_rad: FLOAT, cx: FLOAT, cy: FLOAT| -> ImmutableString {
            rust_motion_paths::rotate_path(&path, angle_rad, cx, cy).into()
        },
    );
    engine.register_fn("reverse_path", |path: ImmutableString| -> ImmutableString {
        rust_motion_paths::reverse_path(&path).into()
    });
}

pub(super) fn context_map(frame: u32, context: NativeCompositionContext) -> Map {
    let mut map = Map::new();
    map.insert("frame".into(), Dynamic::from(frame as INT));
    map.insert(
        "time".into(),
        Dynamic::from((frame as f64) / context.fps.max(0.001)),
    );
    map.insert("width".into(), Dynamic::from(context.width as INT));
    map.insert("height".into(), Dynamic::from(context.height as INT));
    map.insert("fps".into(), Dynamic::from(context.fps as FLOAT));
    map.insert(
        "duration".into(),
        Dynamic::from(context.duration_in_frames as INT),
    );
    map.insert(
        "progress".into(),
        Dynamic::from(context.progress(frame) as FLOAT),
    );
    map
}
