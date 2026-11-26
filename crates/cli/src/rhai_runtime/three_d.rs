//! Scene builder methods for 3D primitives, meshes and glTF models.

use super::*;

impl SceneBuilder {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn cube_3d(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        size: FLOAT,
        pitch: FLOAT,
        yaw: FLOAT,
        roll: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        let mut mesh = rust_motion_rasterizer::Mesh3D::cube(non_negative_f32("size", size)?);
        mesh.rotate(pitch as f32, yaw as f32, roll as f32);
        mesh.render_to_scene(
            &mut self.scene,
            finite_f32("x", x)?,
            finite_f32("y", y)?,
            500.0,
            parse_color(color)?,
            rust_motion_rasterizer::Vec3::new(0.6, 1.0, 0.8),
            false,
        );
        Ok(())
    }

    pub(super) fn sphere_3d(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        radius: FLOAT,
        pitch: FLOAT,
        yaw: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        let mut mesh =
            rust_motion_rasterizer::Mesh3D::sphere(non_negative_f32("radius", radius)?, 12, 16);
        mesh.rotate(pitch as f32, yaw as f32, 0.0);
        mesh.render_to_scene(
            &mut self.scene,
            finite_f32("x", x)?,
            finite_f32("y", y)?,
            500.0,
            parse_color(color)?,
            rust_motion_rasterizer::Vec3::new(0.6, 1.0, 0.8),
            false,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn torus_3d(
        &mut self,
        x: FLOAT,
        y: FLOAT,
        r_major: FLOAT,
        r_minor: FLOAT,
        pitch: FLOAT,
        yaw: FLOAT,
        color: &str,
    ) -> RhaiResult<()> {
        let mut mesh = rust_motion_rasterizer::Mesh3D::torus(
            non_negative_f32("major radius", r_major)?,
            non_negative_f32("minor radius", r_minor)?,
            16,
            12,
        );
        mesh.rotate(pitch as f32, yaw as f32, 0.0);
        mesh.render_to_scene(
            &mut self.scene,
            finite_f32("x", x)?,
            finite_f32("y", y)?,
            500.0,
            parse_color(color)?,
            rust_motion_rasterizer::Vec3::new(0.6, 1.0, 0.8),
            false,
        );
        Ok(())
    }

    pub(super) fn mesh_3d(&mut self, mesh_type: &str, opts: Map) -> RhaiResult<()> {
        let center_x = get_opt_f32(&opts, "x", get_opt_f32(&opts, "center_x", 960.0));
        let center_y = get_opt_f32(&opts, "y", get_opt_f32(&opts, "center_y", 540.0));
        let size = get_opt_f32(&opts, "size", 200.0);
        let scale = get_opt_f32(&opts, "scale", 1.0);
        let pitch = get_opt_f32(&opts, "pitch", get_opt_f32(&opts, "rotate_x", 0.0)).to_radians();
        let yaw = get_opt_f32(&opts, "yaw", get_opt_f32(&opts, "rotate_y", 0.0)).to_radians();
        let roll = get_opt_f32(&opts, "roll", get_opt_f32(&opts, "rotate_z", 0.0)).to_radians();
        let cam_dist = get_opt_f32(&opts, "camera_dist", 600.0);
        let wireframe = opts
            .get("wireframe")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false);

        let color_str = get_opt_string(&opts, "color").unwrap_or_else(|| "#3b82f6".to_string());
        let base_color = parse_color(&color_str)?;

        let light_x = get_opt_f32(&opts, "light_x", 0.5);
        let light_y = get_opt_f32(&opts, "light_y", 1.0);
        let light_z = get_opt_f32(&opts, "light_z", 0.8);
        let light_dir = Vec3::new(light_x, light_y, light_z);

        let mut mesh = match mesh_type.to_lowercase().as_str() {
            "cube" | "box" => Mesh3D::cube(size),
            "sphere" => {
                let rings = get_opt_usize(&opts, "rings", 16);
                let sectors = get_opt_usize(&opts, "sectors", 24);
                Mesh3D::sphere(size * 0.5, rings, sectors)
            }
            "torus" | "donut" => {
                let r_major = size * 0.5;
                let r_minor = get_opt_f32(&opts, "tube_radius", r_major * 0.35);
                let segs_major = get_opt_usize(&opts, "segs_major", 24);
                let segs_minor = get_opt_usize(&opts, "segs_minor", 16);
                Mesh3D::torus(r_major, r_minor, segs_major, segs_minor)
            }
            unknown => {
                return Err(runtime_error(format!(
                    "Unknown 3D mesh type '{unknown}'. Valid types: 'cube', 'sphere', 'torus'"
                )))
            }
        };

        if (scale - 1.0).abs() > 1e-4 {
            mesh.scale(scale);
        }
        mesh.rotate(pitch, yaw, roll);

        mesh.render_to_scene(
            &mut self.scene,
            center_x,
            center_y,
            cam_dist,
            base_color,
            light_dir,
            wireframe,
        );

        Ok(())
    }

    pub(super) fn gltf(&mut self, gltf_source: &str, opts: Map) -> RhaiResult<()> {
        let center_x = get_opt_f32(&opts, "x", get_opt_f32(&opts, "center_x", 960.0));
        let center_y = get_opt_f32(&opts, "y", get_opt_f32(&opts, "center_y", 540.0));
        let scale = get_opt_f32(&opts, "scale", 100.0);
        let pitch = get_opt_f32(&opts, "pitch", get_opt_f32(&opts, "rotate_x", 0.0)).to_radians();
        let yaw = get_opt_f32(&opts, "yaw", get_opt_f32(&opts, "rotate_y", 0.0)).to_radians();
        let roll = get_opt_f32(&opts, "roll", get_opt_f32(&opts, "rotate_z", 0.0)).to_radians();
        let cam_dist = get_opt_f32(&opts, "camera_dist", 600.0);
        let wireframe = opts
            .get("wireframe")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false);

        let light_x = get_opt_f32(&opts, "light_x", 0.5);
        let light_y = get_opt_f32(&opts, "light_y", 1.0);
        let light_z = get_opt_f32(&opts, "light_z", 0.8);
        let light_dir = Vec3::new(light_x, light_y, light_z);

        let trimmed = gltf_source.trim();
        let model = if trimmed.starts_with('{') {
            parse_gltf(trimmed, None)
                .map_err(|e| runtime_error(format!("Failed to parse glTF: {e}")))?
        } else if let Some(b64) = trimmed.strip_prefix("data:model/gltf-binary;base64,") {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| runtime_error(format!("Failed to decode GLB base64: {e}")))?;
            parse_glb(&bytes).map_err(|e| runtime_error(format!("Failed to parse GLB: {e}")))?
        } else if let Some(b64) = trimmed.strip_prefix("data:model/gltf+json;base64,") {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| runtime_error(format!("Failed to decode glTF JSON base64: {e}")))?;
            let json = std::str::from_utf8(&bytes)
                .map_err(|e| runtime_error(format!("Invalid UTF-8 in glTF JSON base64: {e}")))?;
            parse_gltf(json, None)
                .map_err(|e| runtime_error(format!("Failed to parse glTF: {e}")))?
        } else {
            // Attempt JSON parse first, then GLB
            parse_gltf(trimmed, None)
                .or_else(|_| parse_glb(trimmed.as_bytes()))
                .map_err(|e| runtime_error(format!("Failed to parse 3D glTF/GLB: {e}")))?
        };

        let base_color = if let Some(color_str) = get_opt_string(&opts, "color") {
            parse_color(&color_str)?
        } else {
            model.base_color
        };

        let time_sec = if let Some(t) = opts
            .get("time")
            .and_then(|v| v.as_float().ok().map(|f| f as f32))
        {
            t
        } else if let Some(f) = opts.get("frame").and_then(|v| v.as_int().ok()) {
            let fps = opts
                .get("fps")
                .and_then(|v| v.as_float().ok())
                .unwrap_or(30.0) as f32;
            f as f32 / fps
        } else {
            0.0
        };

        let mut mesh = if opts.contains_key("time") || opts.contains_key("frame") {
            model.sample_pose(time_sec)
        } else {
            model.mesh
        };

        if (scale - 1.0).abs() > 1e-4 {
            mesh.scale(scale);
        }
        mesh.rotate(pitch, yaw, roll);

        mesh.render_to_scene(
            &mut self.scene,
            center_x,
            center_y,
            cam_dist,
            base_color,
            light_dir,
            wireframe,
        );

        Ok(())
    }
}
