//! Compilation and semantic tests for the whole compiler pipeline
#![feature(iter_array_chunks)]
#![feature(debug_closure_helpers)]
#![deny(warnings)]
#![deny(missing_docs)]

mod cargo_proj;
mod compiler_test;
pub mod testing;

/// Represents an on-disk Cargo project generated for tests.
pub use self::cargo_proj::Project;
/// Builder for constructing on-disk Cargo projects used by tests.
pub use self::cargo_proj::ProjectBuilder;
/// Generates an on-disk Cargo project in the workspace `target/` directory for use in tests.
pub use self::cargo_proj::project;
pub use self::{
    compiler_test::{CargoTest, CompilerTest, CompilerTestBuilder, RustcTest},
    testing::setup::default_session,
};

#[cfg(test)]
mod codegen;
#[cfg(test)]
mod rust_masm_tests;
