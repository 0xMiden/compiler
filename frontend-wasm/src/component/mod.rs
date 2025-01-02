//! Support for the Wasm component model translation
//!
//! This module contains all of the internal type definitions to parse and
//! translate the component model.

pub mod build_ir;
mod dfg;
pub mod info;
mod inline;
mod parser;
mod translator;
mod translator2;
mod types;

pub use self::{info::*, parser::*, types::*};
