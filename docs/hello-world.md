# Hello World Tutorial: Creating Your First Composition

This tutorial walks through creating, animating, and rendering your first programmatic video composition in Rust Motion.

---

## 1. Composition Architecture

Every Rust Motion video is built on the `Composition` contract:

1. **`prepare(props, context)`**: Evaluates JSON input props against the video dimensions (`width`, `height`), `fps`, and `duration_in_frames`. Resolves static assets and returns a `PreparedComposition`.
2. **`render(frame)`**: A pure, deterministic function that evaluates the given `frame` index and returns a `Scene` (a vector graphic tree of rectangles, text, paths, media, and filters).

---

## 2. Approach A: Writing a Native Rust Composition

Add `dioxuscut-composition`, `dioxuscut-rasterizer`, and `dioxuscut-animation` to your `Cargo.toml`:

```rust
use dioxuscut_animation::interpolate;
use dioxuscut_composition::{
    Composition, CompositionError, NativeCompositionContext, PreparedComposition,
};
use dioxuscut_rasterizer::scene::{Color, Scene, SceneNode};

pub struct TitleCardComposition;

impl Composition for TitleCardComposition {
    fn id(&self) -> &str {
        "TitleCard"
    }

    fn prepare(
        &self,
        props: &serde_json::Value,
        context: NativeCompositionContext,
    ) -> Result<Box<dyn PreparedComposition>, CompositionError> {
        let title = props
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Hello, Rust Motion!")
            .to_string();

        Ok(Box::new(PreparedTitleCard { context, title }))
    }
}

struct PreparedTitleCard {
    context: NativeCompositionContext,
    title: String,
}

impl PreparedComposition for PreparedTitleCard {
    fn render(&self, frame: u32) -> Result<Scene, CompositionError> {
        let mut scene = Scene::new();

        // 1. Animated background color
        let bg_r = interpolate(frame as f64, &[0.0, 60.0], &[15.0, 30.0], Default::default()) as u8;
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w: self.context.width as f32,
            h: self.context.height as f32,
            fill: Color::rgb(bg_r, 20, 40),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        // 2. Animated scale & position
        let scale = interpolate(frame as f64, &[0.0, 30.0], &[0.5, 1.0], Default::default()) as f32;
        let card_w = 400.0 * scale;
        let card_h = 200.0 * scale;
        let card_x = (self.context.width as f32 - card_w) / 2.0;
        let card_y = (self.context.height as f32 - card_h) / 2.0;

        scene.push(SceneNode::Rect {
            x: card_x,
            y: card_y,
            w: card_w,
            h: card_h,
            fill: Color::rgba(255, 255, 255, 220),
            stroke: Some(Color::rgb(0, 180, 255)),
            stroke_width: 3.0,
            corner_radius: 16.0,
        });

        Ok(scene)
    }
}
```

---

## 3. Approach B: Writing a Rhai Script (No Recompilation Needed)

If you are generating videos dynamically or using AI agents, write a Rhai script (`my_video.rhai`):

```rhai
fn render(frame, props, ctx) {
    let title = props.title || "Rust Motion 1.0";
    let progress = interpolate(frame, [0, 45], [0.0, 1.0]);

    let scene = new_scene();

    // Background
    scene.rect(0.0, 0.0, ctx.width, ctx.height, rgb(18, 18, 24));

    // Animated Card
    let y_offset = interpolate(frame, [0, 30], [ctx.height, (ctx.height - 200.0) / 2.0]);
    scene.rect(
        (ctx.width - 600.0) / 2.0,
        y_offset,
        600.0,
        200.0,
        rgba(255, 255, 255, 240),
        12.0 // corner radius
    );

    return scene;
}
```

Create a props file `props.json`:
```json
{
  "title": "Automated Agent Video"
}
```

---

## 4. Validating and Rendering

Validate the script and props before spending GPU/CPU cycles:

```bash
dioxuscut validate --script my_video.rhai --props props.json --json
```

Render to video:

```bash
# Render to MP4
dioxuscut render --script my_video.rhai --props props.json -o out.mp4

# Render to WebM with VP9
dioxuscut render --script my_video.rhai --props props.json --codec vp9 -o out.webm

# Render to Animated GIF
dioxuscut render --script my_video.rhai --props props.json --codec gif -o out.gif
```
