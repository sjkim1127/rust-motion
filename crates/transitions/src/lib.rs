//! # rust-motion-transitions
//!
//! Transition components and presentations — wrap children in animated reveal/exit effects.
//!
//! Equivalent to `@remotion/transitions`.

pub mod clock_wipe;
pub mod dissolve;
pub mod easing;
pub mod fade;
pub mod flip;
pub mod iris;
pub mod linear_wipe;
pub mod presentation;
pub mod scene;
pub mod slide;
pub mod zoom;

pub use clock_wipe::{ClockWipe, ClockWipeProps, SceneClockWipe};
pub use dissolve::{Dissolve, DissolveProps, SceneDissolve};
pub use easing::{
    bezier, ease, ease_in, ease_in_cubic, ease_in_out, ease_in_out_cubic, ease_in_out_quad,
    ease_in_out_sine, ease_in_quad, ease_in_sine, ease_out, ease_out_cubic, ease_out_quad,
    ease_out_sine, linear, EasingFn, LinearTiming, SpringConfig, SpringTiming, TransitionTiming,
};
pub use fade::{Fade, FadePresentation, FadeProps};
pub use flip::{Flip, FlipDirection, FlipProps, SceneFlip};
pub use iris::{Iris, IrisProps, SceneIris};
pub use linear_wipe::{
    LinearWipe, LinearWipe as Wipe, LinearWipeProps, SceneLinearWipe, SceneLinearWipe as SceneWipe,
    WipeDirection,
};
pub use presentation::{PresentationVisual, TransitionContext, TransitionPresentation};
pub use scene::{SceneFade, SceneFade as FadeEmitter, SceneSlide, SceneSlide as SlideEmitter};
pub use slide::{Slide, SlideDirection, SlidePresentation, SlideProps};
pub use zoom::{SceneZoom, Zoom, ZoomMode, ZoomProps};
