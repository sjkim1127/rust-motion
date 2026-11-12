//! CPU Software rasterizer fallback for Scene3D (tiny-skia compatible).

use super::light::Scene3DLight;
use super::material::Material3D;
use super::scene::Scene3D;
use crate::mesh3d::{Mat4, Vec3};
use crate::scene::Color;
use tiny_skia::{Color as SkColor, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// Renders a [`Scene3D`] into a 2D `tiny-skia` pixmap with perspective projection,
/// face sorting, and lighting evaluation.
#[allow(clippy::too_many_arguments)]
pub fn render_scene3d_software(
    scene: &Scene3D,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    opacity: f32,
    pixmap: &mut Pixmap,
    transform: Transform,
) {
    if w <= 0.0 || h <= 0.0 || opacity <= 0.0 {
        return;
    }

    // 1. Draw solid background if configured
    if let super::scene::Scene3DBackground::Solid { color } = scene.background {
        let bg_color = SkColor::from_rgba8(
            color.r,
            color.g,
            color.b,
            (color.a as f32 * opacity).round() as u8,
        );
        let mut paint = Paint::default();
        paint.set_color(bg_color);
        let mut pb = PathBuilder::new();
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            pb.push_rect(rect);
            if let Some(path) = pb.finish() {
                pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
        }
    }

    let items = scene.collect_render_items();
    let cam = &scene.camera;

    // Collect all projected faces across all meshes and instances
    struct ProjectedFace {
        depth: f32,
        points: Vec<(f32, f32)>,
        fill_color: Option<Color>,
        stroke_color: Option<Color>,
        stroke_width: f32,
    }

    let mut projected_faces: Vec<ProjectedFace> = Vec::new();

    for item in items {
        let mesh = item.geometry.to_mesh();
        if mesh.vertices.is_empty() || mesh.faces.is_empty() {
            continue;
        }

        let is_wireframe = matches!(item.material, Material3D::Wireframe(_));
        let wireframe_width = match item.material {
            Material3D::Wireframe(wf) => wf.line_width,
            _ => 1.0,
        };
        let is_double_sided = match item.material {
            Material3D::Pbr(pbr) => pbr.double_sided,
            _ => false,
        };

        // Determine list of model matrices (single or instanced)
        let model_matrices: Vec<(Mat4, Option<Color>)> = if let Some(inst) = item.instancing {
            inst.instances
                .iter()
                .map(|i| {
                    let local = i.position;
                    let local_mat =
                        Mat4::from_translation_rotation_scale(local, i.rotation, i.scale);
                    (item.world_matrix * local_mat, i.color_override)
                })
                .collect()
        } else {
            vec![(item.world_matrix, None)]
        };

        for (model_mat, color_override) in model_matrices {
            let base_color = color_override.unwrap_or_else(|| item.material.base_color());

            // Transform vertices to world space
            let world_vertices: Vec<Vec3> = mesh
                .vertices
                .iter()
                .map(|&v| model_mat.transform_point(v))
                .collect();

            // Project each vertex to sub-viewport [x, x+w] and [y, y+h]
            let mut screen_coords: Vec<Option<(f32, f32, f32)>> =
                Vec::with_capacity(world_vertices.len());
            for &wv in &world_vertices {
                if let Some((sx, sy, depth)) = cam.project_point(wv, w, h) {
                    screen_coords.push(Some((x + sx, y + sy, depth)));
                } else {
                    screen_coords.push(None);
                }
            }

            for face in &mesh.faces {
                if face.len() < 3 {
                    continue;
                }

                // Check if any vertex is clipped
                let mut face_pts = Vec::with_capacity(face.len());
                let mut avg_depth = 0.0f32;
                let mut clipped = false;

                for &idx in face {
                    match screen_coords[idx] {
                        Some((sx, sy, depth)) => {
                            face_pts.push((sx, sy));
                            avg_depth += depth;
                        }
                        None => {
                            clipped = true;
                            break;
                        }
                    }
                }

                if clipped {
                    continue;
                }
                avg_depth /= face.len() as f32;

                // Backface culling via world normal
                let v0 = world_vertices[face[0]];
                let v1 = world_vertices[face[1]];
                let v2 = world_vertices[face[2]];

                let edge1 = Vec3::new(v1.x - v0.x, v1.y - v0.y, v1.z - v0.z);
                let edge2 = Vec3::new(v2.x - v0.x, v2.y - v0.y, v2.z - v0.z);
                let normal = edge1.cross(edge2).normalize();

                let to_cam = Vec3::new(
                    cam.position.x - v0.x,
                    cam.position.y - v0.y,
                    cam.position.z - v0.z,
                )
                .normalize();

                let n_dot_v = normal.dot(to_cam);
                if !is_wireframe && !is_double_sided && n_dot_v <= 0.0 {
                    continue;
                }

                // Evaluate lighting
                let shaded_color = evaluate_face_color(base_color, normal, v0, &scene.lights);

                if is_wireframe {
                    projected_faces.push(ProjectedFace {
                        depth: avg_depth,
                        points: face_pts,
                        fill_color: None,
                        stroke_color: Some(shaded_color),
                        stroke_width: wireframe_width,
                    });
                } else {
                    projected_faces.push(ProjectedFace {
                        depth: avg_depth,
                        points: face_pts,
                        fill_color: Some(shaded_color),
                        stroke_color: Some(Color::rgba(0, 0, 0, 30)),
                        stroke_width: 0.5,
                    });
                }
            }
        }
    }

    // Sort descending by depth (Painter's algorithm: farthest rendered first)
    projected_faces.sort_by(|a, b| {
        b.depth
            .partial_cmp(&a.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Draw all sorted faces
    for face in projected_faces {
        let mut pb = PathBuilder::new();
        for (i, &(px, py)) in face.points.iter().enumerate() {
            if i == 0 {
                pb.move_to(px, py);
            } else {
                pb.line_to(px, py);
            }
        }
        pb.close();

        let Some(path) = pb.finish() else {
            continue;
        };

        // Fill
        if let Some(fill) = face.fill_color {
            let mut paint = Paint::default();
            paint.set_color(SkColor::from_rgba8(
                fill.r,
                fill.g,
                fill.b,
                (fill.a as f32 * opacity).round() as u8,
            ));
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }

        // Stroke
        if let Some(stroke) = face.stroke_color {
            let mut paint = Paint::default();
            paint.set_color(SkColor::from_rgba8(
                stroke.r,
                stroke.g,
                stroke.b,
                (stroke.a as f32 * opacity).round() as u8,
            ));
            let sk_stroke = Stroke {
                width: face.stroke_width,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &sk_stroke, transform, None);
        }
    }
}

fn evaluate_face_color(
    base_color: Color,
    normal: Vec3,
    point: Vec3,
    lights: &[Scene3DLight],
) -> Color {
    if lights.is_empty() {
        return base_color;
    }

    let mut r_acc = 0.0f32;
    let mut g_acc = 0.0f32;
    let mut b_acc = 0.0f32;

    let base_r = base_color.r as f32 / 255.0;
    let base_g = base_color.g as f32 / 255.0;
    let base_b = base_color.b as f32 / 255.0;

    for light in lights {
        match light.light_type {
            super::light::Light3DType::Ambient => {
                let factor = light.intensity;
                r_acc += base_r * (light.color.r as f32 / 255.0) * factor;
                g_acc += base_g * (light.color.g as f32 / 255.0) * factor;
                b_acc += base_b * (light.color.b as f32 / 255.0) * factor;
            }
            super::light::Light3DType::Directional => {
                let l = (-light.direction).normalize();
                let n_dot_l = normal.dot(l).max(0.0);
                let factor = n_dot_l * light.intensity;
                r_acc += base_r * (light.color.r as f32 / 255.0) * factor;
                g_acc += base_g * (light.color.g as f32 / 255.0) * factor;
                b_acc += base_b * (light.color.b as f32 / 255.0) * factor;
            }
            super::light::Light3DType::Point => {
                let to_light = Vec3::new(
                    light.position.x - point.x,
                    light.position.y - point.y,
                    light.position.z - point.z,
                );
                let dist = to_light.length();
                if dist > light.range {
                    continue;
                }
                let l = to_light.normalize();
                let n_dot_l = normal.dot(l).max(0.0);
                let atten = (1.0 - (dist / light.range.max(1.0)))
                    .max(0.0)
                    .powf(light.decay);
                let factor = n_dot_l * light.intensity * atten;
                r_acc += base_r * (light.color.r as f32 / 255.0) * factor;
                g_acc += base_g * (light.color.g as f32 / 255.0) * factor;
                b_acc += base_b * (light.color.b as f32 / 255.0) * factor;
            }
            super::light::Light3DType::Spot => {
                let to_light = Vec3::new(
                    light.position.x - point.x,
                    light.position.y - point.y,
                    light.position.z - point.z,
                );
                let dist = to_light.length();
                if dist > light.range {
                    continue;
                }
                let l = to_light.normalize();
                let spot_cos = (-l).dot(light.direction.normalize());
                let outer_cos = light.outer_cone_angle_deg.to_radians().cos();
                let inner_cos = light.inner_cone_angle_deg.to_radians().cos();

                if spot_cos < outer_cos {
                    continue;
                }
                let spot_factor = if inner_cos > outer_cos {
                    ((spot_cos - outer_cos) / (inner_cos - outer_cos)).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let n_dot_l = normal.dot(l).max(0.0);
                let atten = (1.0 - (dist / light.range.max(1.0)))
                    .max(0.0)
                    .powf(light.decay);
                let factor = n_dot_l * light.intensity * atten * spot_factor;
                r_acc += base_r * (light.color.r as f32 / 255.0) * factor;
                g_acc += base_g * (light.color.g as f32 / 255.0) * factor;
                b_acc += base_b * (light.color.b as f32 / 255.0) * factor;
            }
        }
    }

    Color::rgba(
        (r_acc.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g_acc.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b_acc.clamp(0.0, 1.0) * 255.0).round() as u8,
        base_color.a,
    )
}
