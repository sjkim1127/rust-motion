use crate::{render_virtual_dom, CssError, NativeDomError, Stylesheet};
use dioxus_core::VirtualDom;
use rust_motion_composition::{CompositionError, NativeComposition, NativeCompositionContext};
use rust_motion_rasterizer::Scene;
use serde_json::Value;

/// Creates a fresh Dioxus [`VirtualDom`] for a native video frame.
///
/// A new VDOM is built per frame so the resulting composition remains safe to
/// render concurrently through Rust Motion's native renderer.
pub trait VdomFactory: Send + Sync {
    fn create(&self, frame: u32, props: &Value, context: NativeCompositionContext) -> VirtualDom;
}

impl<F> VdomFactory for F
where
    F: Fn(u32, &Value, NativeCompositionContext) -> VirtualDom + Send + Sync,
{
    fn create(&self, frame: u32, props: &Value, context: NativeCompositionContext) -> VirtualDom {
        self(frame, props, context)
    }
}

/// Adapts a Dioxus VDOM factory to the shared native composition contract.
pub struct VdomComposition<F> {
    id: String,
    factory: F,
    stylesheet: Stylesheet,
}

impl<F> VdomComposition<F> {
    pub fn new(id: impl Into<String>, factory: F) -> Self {
        Self {
            id: id.into(),
            factory,
            stylesheet: Stylesheet::new(),
        }
    }

    pub fn with_stylesheet(mut self, stylesheet: Stylesheet) -> Self {
        self.stylesheet = stylesheet;
        self
    }

    pub fn with_css(self, css: &str) -> Result<Self, CssError> {
        Ok(self.with_stylesheet(Stylesheet::parse(css)?))
    }
}

impl<F> NativeComposition for VdomComposition<F>
where
    F: VdomFactory,
{
    fn id(&self) -> &str {
        &self.id
    }

    fn render(
        &self,
        frame: u32,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Scene, CompositionError> {
        let mut virtual_dom = self.factory.create(frame, props, context);
        render_virtual_dom(
            &mut virtual_dom,
            context.width,
            context.height,
            &self.stylesheet,
        )
        .map_err(|error: NativeDomError| CompositionError::render(frame, error.to_string()))
    }
}
