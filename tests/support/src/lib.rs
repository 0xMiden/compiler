//! Shared support infrastructure for integration tests.
#![feature(debug_closure_helpers)]
#![deny(warnings)]
#![deny(missing_docs)]

/// Utilities for generating on-disk Cargo projects for tests.
pub mod cargo_proj;
/// Compiler test builders and pipeline assertions.
pub mod compiler_test;
/// VM execution, initialization, and session setup helpers.
pub mod testing;

/// Represents an on-disk Cargo project generated for tests.
pub use self::cargo_proj::Project;
/// Builder for constructing on-disk Cargo projects used by tests.
pub use self::cargo_proj::ProjectBuilder;
/// Generates an on-disk Cargo project in the Cargo target directory for use in tests.
pub use self::cargo_proj::project;
pub use self::{
    compiler_test::{CargoTest, CompilerTest, CompilerTestBuilder, RustcTest},
    testing::setup::default_session,
};
