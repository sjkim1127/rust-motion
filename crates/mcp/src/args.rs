//! Typed access to tool arguments: defaults apply only to missing fields, never to malformed ones.

use serde_json::{Map, Value};

pub(crate) struct Args(Map<String, Value>);

impl Args {
    pub(crate) fn new(value: Value) -> Result<Self, String> {
        match value {
            Value::Object(map) => Ok(Self(map)),
            Value::Null => Ok(Self(Map::new())),
            _ => Err("invalid arguments: expected a JSON object".to_string()),
        }
    }

    /// The field, treating an explicit `null` like an absent one.
    fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key).filter(|value| !value.is_null())
    }

    fn typed<'a, T>(
        &'a self,
        key: &str,
        expected: &str,
        convert: impl FnOnce(&'a Value) -> Option<T>,
    ) -> Result<Option<T>, String> {
        match self.get(key) {
            None => Ok(None),
            Some(value) => convert(value)
                .map(Some)
                .ok_or_else(|| format!("invalid '{key}': expected {expected}, got {value}")),
        }
    }

    pub(crate) fn opt_str(&self, key: &str) -> Result<Option<&str>, String> {
        self.typed(key, "a string", Value::as_str)
    }

    pub(crate) fn opt_f64(&self, key: &str) -> Result<Option<f64>, String> {
        self.typed(key, "a finite number", |value| {
            value.as_f64().filter(|number| number.is_finite())
        })
    }

    pub(crate) fn opt_f32(&self, key: &str) -> Result<Option<f32>, String> {
        self.typed(key, "a finite 32-bit number", |value| {
            value
                .as_f64()
                .map(|number| number as f32)
                .filter(|number| number.is_finite())
        })
    }

    pub(crate) fn opt_u32(&self, key: &str) -> Result<Option<u32>, String> {
        self.typed(key, "a non-negative integer up to 4294967295", |value| {
            value.as_u64().and_then(|number| u32::try_from(number).ok())
        })
    }

    fn required<T>(key: &str, value: Option<T>) -> Result<T, String> {
        value.ok_or_else(|| format!("missing '{key}'"))
    }

    pub(crate) fn str(&self, key: &str) -> Result<&str, String> {
        Self::required(key, self.opt_str(key)?)
    }

    pub(crate) fn f64(&self, key: &str) -> Result<f64, String> {
        Self::required(key, self.opt_f64(key)?)
    }

    pub(crate) fn bool(&self, key: &str) -> Result<bool, String> {
        Self::required(key, self.typed(key, "a boolean", Value::as_bool)?)
    }
}
