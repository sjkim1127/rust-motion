//! Composition registry and the set of built-in compositions shipped with the binary.

use crate::builtin::*;
use crate::contract::*;
use std::collections::BTreeMap;
use thiserror::Error;

/// Errors produced while building or querying a composition registry.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompositionRegistryError {
    #[error("Composition '{0}' is already registered")]
    Duplicate(String),
    #[error("Unknown composition '{requested}'. Available compositions: {available}")]
    Unknown {
        requested: String,
        available: String,
    },
}

/// Deterministic registry used by preview and export clients to resolve composition IDs.
#[derive(Default)]
pub struct CompositionRegistry {
    compositions: BTreeMap<String, Box<dyn Composition>>,
}

impl CompositionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<C>(&mut self, composition: C) -> Result<(), CompositionRegistryError>
    where
        C: Composition + 'static,
    {
        let id = composition.id().to_string();
        if self.compositions.contains_key(&id) {
            return Err(CompositionRegistryError::Duplicate(id));
        }
        self.compositions.insert(id, Box::new(composition));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<&dyn Composition, CompositionRegistryError> {
        self.compositions.get(id).map(Box::as_ref).ok_or_else(|| {
            CompositionRegistryError::Unknown {
                requested: id.to_string(),
                available: self.ids().join(", "),
            }
        })
    }

    pub fn ids(&self) -> Vec<&str> {
        self.compositions.keys().map(String::as_str).collect()
    }
}

/// Registry shipped by the standalone `rust-motion` binary.
pub fn built_in_registry() -> CompositionRegistry {
    let mut registry = CompositionRegistry::new();
    registry
        .register(HelloWorldComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(NamedTimeEventsDemo)
        .expect("built-in composition IDs must be unique");
    registry
        .register(SpringRectsComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(ShapesAndFiltersComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(CyberpunkGridComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(ComplexGradientsComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(PodcastWaveformComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(KaraokeCaptionsComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(BarChartRaceComposition)
        .expect("built-in composition IDs must be unique");
    registry
        .register(CodeTerminalComposition)
        .expect("built-in composition IDs must be unique");
    registry
}
