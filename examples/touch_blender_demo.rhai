//! "Remotion + Blender + TouchDesigner" Paradigm Showcase
//! Demonstrates native 3D geometry (SOP), LFO/Lag modulation (CHOP), and timeline color grading.

fn render(ctx, props) {
    let frame = ctx.frame.to_float();
    let fps = ctx.fps;

    // 1. TouchDesigner CHOP: Continuous periodic wave modulation
    let osc_y = lfo(frame, fps, 0.5, 40.0); // 0.5 Hz, +-40px floating oscillation
    let rot_speed = frame * 0.04;
    let pulse = lfo(frame, fps, 1.0, 15.0);

    // 2. Remotion dynamic color interpolation
    let cube_color = interpolate_colors("#ff007f", "#00f0ff", (frame / 60.0).clamp(0.0, 1.0));
    let torus_color = interpolate_colors("#ffaa00", "#7928ca", (frame / 60.0).clamp(0.0, 1.0));

    let output = scene();

    // Deep space cinematic background
    output.rect(0.0, 0.0, ctx.width.to_float(), ctx.height.to_float(), "#070913");

    // Glowing atmospheric horizon line
    output.rect(0.0, 600.0, ctx.width.to_float(), 2.0, "#00f0ff33");

    // 3. Blender SOP: Native 3D Perspective Meshes
    // Left: 3D Orbiting Torus (donut)
    output.torus_3d(400.0, 540.0 - osc_y, 90.0 + pulse, 28.0, rot_speed * 1.2, rot_speed * 0.8, torus_color);

    // Center: 3D Rotating Cube with dynamic lighting
    output.cube_3d(960.0, 520.0 + osc_y, 140.0, rot_speed, rot_speed * 1.5, rot_speed * 0.5, cube_color);

    // Right: 3D Wireframe/Shaded Sphere
    output.sphere_3d(1520.0, 540.0 - osc_y * 0.8, 85.0, rot_speed * 0.9, rot_speed * 1.1, "#00f0ff");

    // Title Card
    output.text_bold(960.0 - 450.0, 220.0, "RUST_MOTION SYNTHESIS", 64.0, "#ffffff");
    output.text(960.0 - 320.0, 290.0, "Remotion Timeline + Blender 3D + TouchDesigner CHOPs", 24.0, "#00f0ff");

    output
}
