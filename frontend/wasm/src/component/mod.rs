//! Support for the Wasm component model translation
//!
//! This module contains all of the internal type definitions to parse and
//! translate the component model.

pub(crate) mod build_ir;
mod flat;
mod lift_exports;
pub(crate) mod lower_imports;
mod parser;
mod shim_bypass;
mod translator;
mod types;

pub use self::{parser::*, types::*};
