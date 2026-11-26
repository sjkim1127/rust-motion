//! Internal Render IR: resource-oriented graph representation, validation,
//! deterministic topological scheduling, scene lowering, and narrow backend
//! execution contract.

#![allow(dead_code)]

pub(crate) mod executor;
pub(crate) mod lower;
pub(crate) mod types;
pub(crate) mod validate;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use executor::*;
#[allow(unused_imports)]
pub(crate) use lower::*;
#[allow(unused_imports)]
pub(crate) use types::*;
#[allow(unused_imports)]
pub(crate) use validate::*;
