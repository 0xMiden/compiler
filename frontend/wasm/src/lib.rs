//! Performs translation from Wasm to MidenIR

// Coding conventions
#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// Allow unused code that we're going to need for implementing the missing Wasm features (call_direct, tables, etc.)
#![allow(dead_code)]
#![feature(iterator_try_collect)]

extern crate alloc;

mod callable;
mod code_translator;
mod component;
mod config;
mod error;
mod intrinsics;
mod miden_abi;
mod module;
mod ssa;
mod translation_utils;

use alloc::rc::Rc;

use component::build_ir::translate_component;
use error::WasmResult;
use midenc_hir::{Context, dialects::builtin};
use module::build_ir::translate_module_as_component;
use wasmparser::WasmFeatures;

pub use self::{config::*, error::WasmError};

/// The output of the frontend Wasm translation stage
pub struct FrontendOutput {
    /// The IR component translated from the Wasm
    pub component: builtin::ComponentRef,
    /// The serialized AccountComponentMetadata (name, description, storage layout, etc.)
    pub account_component_metadata_bytes: Option<Vec<u8>>,
}

/// Translate a valid Wasm core module or Wasm Component Model binary into Miden
/// IR Component
pub fn translate(
    wasm: &[u8],
    config: &WasmTranslationConfig,
    context: Rc<Context>,
) -> WasmResult<FrontendOutput> {
    if wasm[4..8] == [0x01, 0x00, 0x00, 0x00] {
        // Wasm core module
        // see https://github.com/WebAssembly/component-model/blob/main/design/mvp/Binary.md#component-definitions
        let component = translate_module_as_component(wasm, config, context)?;
        Ok(FrontendOutput {
            component,
            account_component_metadata_bytes: None,
        })
    } else {
        translate_component(wasm, config, context)
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
        | WasmFeatures::WIDE_ARITHMETIC
}

/// The extended set of WebAssembly features which are enabled when working with the Wasm Component
/// Model
pub(crate) fn supported_component_model_features() -> WasmFeatures {
    supported_features() | WasmFeatures::COMPONENT_MODEL
}
