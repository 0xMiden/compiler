//! Support for the Wasm component model translation
//!
//! This module contains all of the internal type definitions to parse and
//! translate the component model.

pub mod build_ir;
mod parser;
mod translator;
mod types;

pub use self::{parser::*, types::*};
