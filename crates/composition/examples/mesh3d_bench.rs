//! Small release-mode benchmark for the cached native 3D composition path.

use rust_motion_composition::{
    NativeCompositionContext, SceneEmitter, SceneFrameContext, SceneMesh3D,
};
use rust_motion_rasterizer::{Color, Scene, Vec3};
use serde_json::Value;
use std::time::Instant;

fn main() {
    let frames = std::env::var("FRAMES")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(300);
    let context = NativeCompositionContext {
        width: 1280,
        height: 720,
        fps: 30.0,
        duration_in_frames: frames,
    };
    let emitter = SceneMesh3D::torus(640.0, 360.0, 140.0, Color::rgb(73, 220, 177))
        .with_rotation_per_frame(Vec3::new(0.008, 0.011, 0.003));

    let start = Instant::now();
    let mut nodes = 0usize;
    for frame in 0..frames {
        let mut scene = Scene::new();
        emitter
            .emit(
                SceneFrameContext::new(frame, context),
                &Value::Null,
                &mut scene,
            )
            .expect("mesh emission failed");
        nodes += scene.nodes.len();
    }
    let elapsed = start.elapsed();
    println!(
        "frames={frames} elapsed_ms={:.3} fps={:.2} avg_nodes={:.1}",
        elapsed.as_secs_f64() * 1000.0,
        frames as f64 / elapsed.as_secs_f64(),
        nodes as f64 / frames.max(1) as f64,
    );
}
