//! Compilation and semantic tests for the whole compiler pipeline
#![deny(warnings)]
#![deny(missing_docs)]

pub use midenc_integration_test_support::{
    CargoTest, CompilerTest, CompilerTestBuilder, Project, ProjectBuilder, RustcTest, cargo_proj,
    compiler_test, default_session, project, testing,
};

#[cfg(test)]
mod codegen;
#[cfg(test)]
mod rust_pipeline;
