//! The compiled `RhaiComposition` and its prepared per-frame renderer.

use super::*;

/// A compiled Rhai composition. Constructing this type compiles the script once.
pub struct RhaiComposition {
    id: String,
    engine: Engine,
    ast: AST,
}

impl RhaiComposition {
    pub fn from_file(path: &Path) -> Result<Self, CompositionError> {
        let source = fs::read_to_string(path).map_err(|error| {
            CompositionError::Prepare(format!(
                "failed to read Rhai script {}: {error}",
                path.display()
            ))
        })?;
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("RhaiComposition");
        Self::from_source(id, &source)
    }

    pub fn from_source(id: impl Into<String>, source: &str) -> Result<Self, CompositionError> {
        let mut engine = hardened_engine();
        register_scene_api(&mut engine);
        let ast = engine
            .compile(source)
            .map_err(|error| CompositionError::Prepare(format!("Rhai compile error: {error}")))?;

        Ok(Self {
            id: id.into(),
            engine,
            ast,
        })
    }
}

impl Composition for RhaiComposition {
    fn id(&self) -> &str {
        &self.id
    }

    fn prepare(
        &self,
        props: &Value,
        context: NativeCompositionContext,
    ) -> Result<Box<dyn PreparedComposition + '_>, CompositionError> {
        let props = rhai::serde::to_dynamic(props).map_err(|error| {
            CompositionError::Prepare(format!("failed to convert JSON props to Rhai: {error}"))
        })?;

        Ok(Box::new(PreparedRhaiComposition {
            engine: &self.engine,
            ast: &self.ast,
            props,
            context,
        }))
    }
}

struct PreparedRhaiComposition<'a> {
    engine: &'a Engine,
    ast: &'a AST,
    props: Dynamic,
    context: NativeCompositionContext,
}

impl PreparedComposition for PreparedRhaiComposition<'_> {
    fn render(&self, frame: u32) -> Result<Scene, CompositionError> {
        let mut scope = Scope::new();
        let context = context_map(frame, self.context);
        let builder = self
            .engine
            .call_fn::<SceneBuilder>(
                &mut scope,
                self.ast,
                "render",
                (context, self.props.clone()),
            )
            .map_err(|error| CompositionError::render(frame, format!("Rhai error: {error}")))?;
        Ok(builder.into_scene())
    }
}
