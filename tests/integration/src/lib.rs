//! Compilation and semantic tests for the whole compiler pipeline
#![deny(warnings)]

pub use midenc_integration_test_support::{
    self as support, CargoTest, CompilerTest, CompilerTestBuilder, Project, ProjectBuilder,
    RustcTest, cargo_proj, compiler_test, default_session, project, testing,
};

#[cfg(test)]
mod assert_helpers;
#[cfg(test)]
mod codegen;
#[cfg(test)]
mod end_to_end;
#[cfg(test)]
mod sdk;
