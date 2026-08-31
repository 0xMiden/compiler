//! Performs translation from Rust MIR to Miden IR.
//!
//! The frontend reads Rust MIR in-process through `rustc_public`, the stable-MIR API, and
//! builds Miden IR from it. It does not go through WebAssembly.
//!
//! # Status
//!
//! This frontend is an experiment. It supports a small part of Rust MIR: integer and boolean
//! types, wrapping addition, moves and copies of whole locals, integer constants, and the
//! `return` terminator. Every other MIR construct causes an error that names the construct.

// Coding conventions
#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// Required to link against the private compiler crates below.
#![feature(rustc_private)]

// `rustc_public` is the API this frontend reads MIR through. The remaining crates are needed
// because the `rustc_public::run!` driver macro expands to references into them.
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_public;
extern crate rustc_public_bridge;

mod config;
mod driver;
#[cfg(test)]
mod tests;
mod translator;

use midenc_hir::dialects::builtin;

pub use self::{config::RustMirTranslationConfig, driver::translate};

/// The output of the frontend Rust MIR translation stage.
pub struct FrontendOutput {
    /// The IR component translated from the Rust MIR.
    pub component: builtin::ComponentRef,
}
