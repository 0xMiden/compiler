//! Compilation and semantic tests for the whole compiler pipeline
#![feature(iter_array_chunks)]
#![feature(debug_closure_helpers)]
//#![deny(warnings)]
#![deny(missing_docs)]

mod cargo_proj;
mod compiler_test;
pub mod testing;

pub use self::{
    compiler_test::{CargoTest, CompilerTest, CompilerTestBuilder, RustcTest},
    testing::setup::default_session,
};

#[cfg(test)]
mod codegen;
#[cfg(test)]
mod rust_masm_tests;
