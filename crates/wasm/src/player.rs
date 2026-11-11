//! WebAssembly real-time browser timeline player and canvas renderer.

use rust_motion_composition::{
    built_in_registry, format_timecode_frames, CompositionRegistry, FrameRate, NativeComposition,
    NativeCompositionContext,
};
use rust_motion_noise::{fbm_2d, FbmOptions};
use rust_motion_rasterizer::{
    backend::FrameConfig, backend::RasterizerBackend, Color, Scene, SceneNode, TinySkiaBackend,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

/// Procedural TouchDesigner-style generative composition for real-time visual synthesis.
pub struct TouchDesignerGenerativeComposition;

impl NativeComposition for TouchDesignerGenerativeComposition {
    fn id(&self) -> &str {
        "TouchDesignerGenerative"
    }

    fn render(
        &self,
        frame: u32,
        props: &serde_json::Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, rust_motion_composition::CompositionError> {
        let mut scene = Scene::new();
        let w = context.width as f32;
        let h = context.height as f32;
        let t = frame as f64 / context.fps.max(1.0);

        // Background
        scene.push(SceneNode::Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
            fill: Color::rgb(10, 12, 22),
            stroke: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
        });

        // Dynamic concentric generative rings & particle nodes
        let cx = w / 2.0;
        let cy = h / 2.0;
        let base_radius = (w.min(h) * 0.18) as f64;
        let ring_count = 12;

        let fbm_opts = FbmOptions {
            octaves: 3,
            lacunarity: 2.0,
            persistence: 0.5,
        };

        for r in 1..=ring_count {
            let radius_step = base_radius + (r as f64 * 16.0);
            let particle_count = (r * 8).min(64);

            for p in 0..particle_count {
                let angle = (p as f64 / particle_count as f64) * std::f64::consts::TAU;
                let noise_offset = fbm_2d(
                    "ring",
                    angle.cos() * 1.5 + t * 0.5,
                    angle.sin() * 1.5 + r as f64 * 0.2,
                    &fbm_opts,
                );
                let current_radius = radius_step + noise_offset * 25.0;

                let px = cx + (current_radius * angle.cos()) as f32;
                let py = cy + (current_radius * angle.sin()) as f32;

                let size = (3.0 + (noise_offset * 4.0).abs()) as f32;
                let hue_shift = (t * 20.0 + r as f64 * 15.0 + p as f64 * 5.0) % 360.0;

                // Color mapping
                let red = ((hue_shift / 360.0 * 255.0) as u8).max(40);
                let green = (140.0 + 80.0 * (t * 2.0 + r as f64).sin()) as u8;
                let blue = 240;

                scene.push(SceneNode::Circle {
                    cx: px,
                    cy: py,
                    r: size,
                    fill: Color::rgba(red, green, blue, 210),
                    stroke: None,
                    stroke_width: 0.0,
                });
            }
        }

        // Center HUD Core
        let core_pulse = (t * 4.0).sin().abs() as f32 * 12.0;
        scene.push(SceneNode::Circle {
            cx,
            cy,
            r: 28.0 + core_pulse,
            fill: Color::rgba(56, 189, 248, 240),
            stroke: Some(Color::rgb(255, 255, 255)),
            stroke_width: 2.0,
        });

        // Overlay title
        let title = props
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("TouchDesigner TOP Canvas (WASM)");

        scene.push(SceneNode::Text {
            content: title.to_string(),
            x: cx - (w * 0.2),
            y: cy + (w.min(h) * 0.38),
            font_size: (w * 0.024).clamp(16.0, 32.0),
            color: Color::rgb(241, 245, 249),
            font_weight: 700,
            font_sources: Vec::new(),
        });

        Ok(scene)
    }
}

/// WebAssembly Player attached to an HTML5 `<canvas>`.
///
/// Provides sub-16ms synchronous frame rendering, instant timeline scrubbing,
/// play/pause/loop controls, composition switching, and dynamic JSON props.
#[wasm_bindgen]
pub struct WasmPlayer {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    backend: TinySkiaBackend,
    registry: CompositionRegistry,
    current_composition_id: String,
    width: u32,
    height: u32,
    fps: f64,
    duration_in_frames: u32,
    current_frame: u32,
    is_playing: bool,
    is_looping: bool,
    playback_rate: f64,
    props: serde_json::Value,
    last_render_time_ms: f64,
    last_tick_ms: Option<f64>,
    accumulated_ms: f64,
}

#[wasm_bindgen]
impl WasmPlayer {
    /// Initialize a new WASM player attached to the specified HTML canvas ID.
    #[wasm_bindgen(constructor)]
    pub fn new(
        canvas_id: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration_in_frames: u32,
    ) -> Result<WasmPlayer, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no global `window`"))?;
        let document = window
            .document()
            .ok_or_else(|| JsValue::from_str("no `document` on window"))?;
        let element = document
            .get_element_by_id(canvas_id)
            .ok_or_else(|| JsValue::from_str(&format!("canvas '#{canvas_id}' not found")))?;

        let canvas: HtmlCanvasElement = element
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|_| JsValue::from_str("element is not an HtmlCanvasElement"))?;

        canvas.set_width(width);
        canvas.set_height(height);

        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("failed to get 2d context"))?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsValue::from_str("failed to cast context"))?;

        let backend = TinySkiaBackend::new();

        let mut registry = built_in_registry();
        registry
            .register(TouchDesignerGenerativeComposition)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let initial_comp = if registry.ids().contains(&"HelloWorld") {
            "HelloWorld".to_string()
        } else {
            registry
                .ids()
                .first()
                .copied()
                .unwrap_or("HelloWorld")
                .to_string()
        };

        let mut player = Self {
            canvas,
            ctx,
            backend,
            registry,
            current_composition_id: initial_comp,
            width,
            height,
            fps: if fps > 0.0 { fps } else { 30.0 },
            duration_in_frames: if duration_in_frames > 0 {
                duration_in_frames
            } else {
                150
            },
            current_frame: 0,
            is_playing: false,
            is_looping: true,
            playback_rate: 1.0,
            props: serde_json::json!({}),
            last_render_time_ms: 0.0,
            last_tick_ms: None,
            accumulated_ms: 0.0,
        };

        // Render initial frame 0
        let _ = player.render_current_frame();

        Ok(player)
    }

    /// Seek immediately to the specified frame and render onto the canvas.
    pub fn seek(&mut self, frame: u32) -> Result<(), JsValue> {
        let max_frame = self.duration_in_frames.saturating_sub(1);
        self.current_frame = frame.min(max_frame);
        self.render_current_frame()
    }

    /// Render a specific frame without modifying the internal current frame state.
    pub fn render_frame(&mut self, frame: u32) -> Result<(), JsValue> {
        let max_frame = self.duration_in_frames.saturating_sub(1);
        let target_frame = frame.min(max_frame);
        self.render_frame_internal(target_frame)
    }

    /// Internal renderer for a frame.
    fn render_frame_internal(&mut self, frame: u32) -> Result<(), JsValue> {
        let perf = web_sys::window().and_then(|w| w.performance());
        let t0 = perf.as_ref().map(|p| p.now()).unwrap_or(0.0);

        let comp = self
            .registry
            .get(&self.current_composition_id)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let context = NativeCompositionContext {
            width: self.width,
            height: self.height,
            fps: self.fps,
            duration_in_frames: self.duration_in_frames,
        };

        let prepared = comp
            .prepare(&self.props, context)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let scene = prepared
            .render(frame)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let config = FrameConfig::new(self.width, self.height, frame, self.fps);
        let rgba = self
            .backend
            .render_frame(&scene, &config)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(rgba.as_raw()),
            self.width,
            self.height,
        )?;

        self.ctx.put_image_data(&image_data, 0.0, 0.0)?;

        let t1 = perf.as_ref().map(|p| p.now()).unwrap_or(0.0);
        self.last_render_time_ms = t1 - t0;

        Ok(())
    }

    /// Re-render the current frame onto the canvas.
    pub fn render_current_frame(&mut self) -> Result<(), JsValue> {
        self.render_frame_internal(self.current_frame)
    }

    /// Start playback.
    pub fn play(&mut self) {
        self.is_playing = true;
        self.last_tick_ms = None;
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.is_playing = false;
        self.last_tick_ms = None;
    }

    /// Toggle play / pause state. Returns new `is_playing` state.
    pub fn toggle(&mut self) -> bool {
        if self.is_playing {
            self.pause();
        } else {
            self.play();
        }
        self.is_playing
    }

    /// Returns `true` if playback is currently active.
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// Configure whether playback loops when reaching the end of the duration.
    pub fn set_loop(&mut self, looping: bool) {
        self.is_looping = looping;
    }

    /// Returns `true` if loop is enabled.
    pub fn is_loop(&self) -> bool {
        self.is_looping
    }

    /// Set playback rate multiplier (e.g. 1.0 = normal, 0.5 = half speed, 2.0 = double speed).
    pub fn set_playback_rate(&mut self, rate: f64) {
        if rate > 0.0 {
            self.playback_rate = rate;
        }
    }

    /// Get current playback rate.
    pub fn get_playback_rate(&self) -> f64 {
        self.playback_rate
    }

    /// Tick called by `requestAnimationFrame`. Automatically advances frames when playing.
    ///
    /// Returns the current frame number.
    pub fn tick(&mut self, timestamp_ms: f64) -> Result<u32, JsValue> {
        if !self.is_playing {
            self.last_tick_ms = Some(timestamp_ms);
            return Ok(self.current_frame);
        }

        let dt_ms = match self.last_tick_ms {
            Some(last) if timestamp_ms >= last => (timestamp_ms - last).min(100.0),
            _ => 1000.0 / self.fps,
        };
        self.last_tick_ms = Some(timestamp_ms);

        self.accumulated_ms += dt_ms * self.playback_rate;
        let frame_duration_ms = 1000.0 / self.fps;

        let mut advanced = false;
        while self.accumulated_ms >= frame_duration_ms {
            self.accumulated_ms -= frame_duration_ms;
            let next_frame = self.current_frame + 1;

            if next_frame >= self.duration_in_frames {
                if self.is_looping {
                    self.current_frame = 0;
                    advanced = true;
                } else {
                    self.current_frame = self.duration_in_frames.saturating_sub(1);
                    self.is_playing = false;
                    advanced = true;
                    break;
                }
            } else {
                self.current_frame = next_frame;
                advanced = true;
            }
        }

        if advanced {
            self.render_current_frame()?;
        }

        Ok(self.current_frame)
    }

    /// Returns the active frame index (0-indexed).
    pub fn get_current_frame(&self) -> u32 {
        self.current_frame
    }

    /// Returns total duration in frames.
    pub fn get_duration_in_frames(&self) -> u32 {
        self.duration_in_frames
    }

    /// Set total duration in frames.
    pub fn set_duration_in_frames(&mut self, duration: u32) {
        if duration > 0 {
            self.duration_in_frames = duration;
            if self.current_frame >= duration {
                self.current_frame = duration - 1;
            }
        }
    }

    /// Returns current timeline FPS.
    pub fn get_fps(&self) -> f64 {
        self.fps
    }

    /// Set timeline FPS.
    pub fn set_fps(&mut self, fps: f64) {
        if fps > 0.0 {
            self.fps = fps;
        }
    }

    /// Returns canvas pixel width.
    pub fn get_width(&self) -> u32 {
        self.width
    }

    /// Returns canvas pixel height.
    pub fn get_height(&self) -> u32 {
        self.height
    }

    /// Resize canvas dimensions.
    pub fn set_size(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
        self.width = width;
        self.height = height;
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.render_current_frame()
    }

    /// Returns SMPTE timecode string for the current frame (e.g. `00:00:02:14`).
    pub fn get_timecode(&self) -> String {
        format_timecode_frames(
            self.current_frame as i64,
            FrameRate::from_f64(self.fps),
            false,
        )
    }

    /// Returns the exact rasterization time in milliseconds for the last rendered frame.
    pub fn get_last_render_time_ms(&self) -> f64 {
        self.last_render_time_ms
    }

    /// Returns the ID of the currently active composition.
    pub fn get_composition(&self) -> String {
        self.current_composition_id.clone()
    }

    /// Switch active composition by ID.
    pub fn set_composition(&mut self, id: &str) -> Result<(), JsValue> {
        if !self.registry.ids().contains(&id) {
            return Err(JsValue::from_str(&format!(
                "Unknown composition '{id}'. Available: {}",
                self.registry.ids().join(", ")
            )));
        }

        self.current_composition_id = id.to_string();
        self.render_current_frame()
    }

    /// Returns a list of all available composition IDs in the registry.
    pub fn get_available_compositions(&self) -> js_sys::Array {
        let array = js_sys::Array::new();
        for id in self.registry.ids() {
            array.push(&JsValue::from_str(id));
        }
        array
    }

    /// Set dynamic JSON props for live parameter updates.
    pub fn set_props(&mut self, json_str: &str) -> Result<(), JsValue> {
        let val: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.props = val;
        self.render_current_frame()
    }

    /// Returns the current dynamic props as a JSON string.
    pub fn get_props(&self) -> String {
        self.props.to_string()
    }

    /// Render a frame and return its raw RGBA byte buffer to JavaScript.
    ///
    /// Ideal for WebCodecs MP4 encoding, WebGL texture uploading, or taking screenshots.
    pub fn get_frame_rgba(&mut self, frame: u32) -> Result<js_sys::Uint8Array, JsValue> {
        let comp = self
            .registry
            .get(&self.current_composition_id)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let context = NativeCompositionContext {
            width: self.width,
            height: self.height,
            fps: self.fps,
            duration_in_frames: self.duration_in_frames,
        };

        let prepared = comp
            .prepare(&self.props, context)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let scene = prepared
            .render(frame)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let config = FrameConfig::new(self.width, self.height, frame, self.fps);
        let rgba = self
            .backend
            .render_frame(&scene, &config)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let slice = rgba.as_raw();
        let uint8_array = js_sys::Uint8Array::new_with_length(slice.len() as u32);
        uint8_array.copy_from(slice);

        Ok(uint8_array)
    }
}
