//! Performs translation from Wasm to MidenIR

// Coding conventions
#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// TODO: remove when completed
#![allow(unused)]

extern crate alloc;

mod code_translator;
mod component;
mod config;
mod error;
mod intrinsics;
mod miden_abi;
mod module;
mod ssa;
mod translation_utils;

#[cfg(test)]
mod test_utils;

use std::rc::Rc;

use component::build_ir::translate_component;
use error::WasmResult;
use midenc_hir::{dialects::builtin, Context};
use module::build_ir::translate_module_as_component;
use wasmparser::WasmFeatures;

pub use self::{config::*, error::WasmError};

/// Translate a valid Wasm core module or Wasm Component Model binary into Miden
/// IR Component
pub fn translate(
    wasm: &[u8],
    config: &WasmTranslationConfig,
    context: Rc<Context>,
) -> WasmResult<builtin::ComponentRef> {
    if wasm[4..8] == [0x01, 0x00, 0x00, 0x00] {
        // Wasm core module
        // see https://github.com/WebAssembly/component-model/blob/main/design/mvp/Binary.md#component-definitions
        translate_module_as_component(wasm, config, context)
    } else {
        todo!()
        // translate_component(wasm, config, context)
    }
}

/// The set of core WebAssembly features which we need to or wish to support
pub(crate) fn supported_features() -> WasmFeatures {
    WasmFeatures::BULK_MEMORY
        | WasmFeatures::FLOATS
        | WasmFeatures::FUNCTION_REFERENCES
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::MUTABLE_GLOBAL
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::TAIL_CALL
}

/// The extended set of WebAssembly features which are enabled when working with the Wasm Component
/// Model
pub(crate) fn supported_component_model_features() -> WasmFeatures {
    supported_features() | WasmFeatures::COMPONENT_MODEL
}
