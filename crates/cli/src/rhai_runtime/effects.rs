//! Scene builder methods for confetti and shader effects.

use super::*;

impl SceneBuilder {
    pub(super) fn confetti_at(&mut self, x: Dynamic, y: Dynamic, opts: Map) -> RhaiResult<()> {
        let x = dynamic_to_finite_f32(&x, "x")?;
        let y = dynamic_to_finite_f32(&y, "y")?;
        let count = get_opt_usize(&opts, "count", 150);
        let spread = get_opt_f32(&opts, "spread", 70.0);
        let angle = get_opt_f32(&opts, "angle", 270.0);
        let velocity = get_opt_f32(&opts, "velocity", 600.0);
        let gravity = get_opt_f32(&opts, "gravity", 750.0);
        let frame = get_opt_usize(&opts, "frame", 0);
        let fps = get_opt_f32(&opts, "fps", 30.0);
        let seed = get_opt_u64(&opts, "seed", 42);

        let colors =
            if let Some(c_arr) = opts.get("colors").and_then(|v| v.clone().into_array().ok()) {
                let mut list = Vec::new();
                for c in c_arr {
                    if let Ok(s) = c.into_string() {
                        if let Ok(parsed) = parse_color(&s) {
                            list.push(parsed);
                        }
                    }
                }
                if list.is_empty() {
                    None
                } else {
                    Some(list)
                }
            } else {
                None
            };

        let shapes =
            if let Some(s_arr) = opts.get("shapes").and_then(|v| v.clone().into_array().ok()) {
                let mut list = Vec::new();
                for s in s_arr {
                    if let Ok(name) = s.into_string() {
                        match name.to_lowercase().as_str() {
                            "circle" => list.push(ConfettiShape::Circle),
                            "star" => list.push(ConfettiShape::Star),
                            "rect" | "rectangle" => list.push(ConfettiShape::Rectangle),
                            _ => {}
                        }
                    }
                }
                if list.is_empty() {
                    None
                } else {
                    Some(list)
                }
            } else {
                None
            };

        let mut emitter = ConfettiEmitter::new(x, y)
            .with_count(count)
            .with_spread(spread)
            .with_angle(angle)
            .with_velocity(velocity);
        emitter.gravity = gravity;
        emitter.seed = seed;
        if let Some(c) = colors {
            emitter = emitter.with_colors(c);
        }
        if let Some(s) = shapes {
            emitter.shapes = s;
        }

        let nodes = emitter.render_frame(frame, fps);
        self.scene.nodes.extend(nodes);
        Ok(())
    }

    pub(super) fn confetti(&mut self, opts: Map) -> RhaiResult<()> {
        let x = Dynamic::from_float(get_opt_f32(&opts, "x", 960.0) as FLOAT);
        let y = Dynamic::from_float(get_opt_f32(&opts, "y", 540.0) as FLOAT);
        self.confetti_at(x, y, opts)
    }

    pub(super) fn confetti_cannon(&mut self, origin: &str, mut opts: Map) -> RhaiResult<()> {
        let (x, y, angle) = match origin.to_lowercase().as_str() {
            "bottom_left" | "left" => (0.0, 1080.0, 315.0),
            "bottom_right" | "right" => (1920.0, 1080.0, 225.0),
            "top" => (960.0, 0.0, 90.0),
            _ => (960.0, 540.0, 270.0),
        };
        if !opts.contains_key("angle") {
            opts.insert("angle".into(), Dynamic::from_float(angle as FLOAT));
        }
        self.confetti_at(
            Dynamic::from_float(x as FLOAT),
            Dynamic::from_float(y as FLOAT),
            opts,
        )
    }

    pub(super) fn shader(&mut self, source: &str, opts: Map) -> RhaiResult<()> {
        let x = get_opt_f32(&opts, "x", 0.0);
        let y = get_opt_f32(&opts, "y", 0.0);
        let w = get_opt_f32(&opts, "w", get_opt_f32(&opts, "width", 1920.0));
        let h = get_opt_f32(&opts, "h", get_opt_f32(&opts, "height", 1080.0));
        let time = get_opt_f32(&opts, "time", 0.0);
        let opacity = get_opt_f32(&opts, "opacity", 1.0);

        let p0 = get_opt_f32(&opts, "p0", get_opt_f32(&opts, "param0", 1.0));
        let p1 = get_opt_f32(&opts, "p1", get_opt_f32(&opts, "param1", 1.0));
        let p2 = get_opt_f32(&opts, "p2", get_opt_f32(&opts, "param2", 1.0));
        let p3 = get_opt_f32(&opts, "p3", get_opt_f32(&opts, "param3", 1.0));

        let params = if let Some(arr) = opts.get("params").and_then(|v| v.clone().into_array().ok())
        {
            [
                arr.first()
                    .and_then(|v| v.as_float().ok())
                    .map(|f| f as f32)
                    .unwrap_or(p0),
                arr.get(1)
                    .and_then(|v| v.as_float().ok())
                    .map(|f| f as f32)
                    .unwrap_or(p1),
                arr.get(2)
                    .and_then(|v| v.as_float().ok())
                    .map(|f| f as f32)
                    .unwrap_or(p2),
                arr.get(3)
                    .and_then(|v| v.as_float().ok())
                    .map(|f| f as f32)
                    .unwrap_or(p3),
            ]
        } else {
            [p0, p1, p2, p3]
        };

        self.scene.push(SceneNode::Shader {
            x,
            y,
            w,
            h,
            source: source.to_string(),
            time,
            params,
            opacity,
        });

        Ok(())
    }
}
