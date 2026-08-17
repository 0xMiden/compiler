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
mod emit;
mod error;
mod fpi;
mod intrinsics;
mod miden_abi;
mod module;
mod ssa;
mod translation_utils;

use alloc::rc::Rc;

use component::build_ir::translate_component;
use error::WasmResult;
use midenc_frontend_wasm_metadata::{
    PackageSections, WASM_NOTE_STORAGE_SCHEMA_CUSTOM_SECTION_NAME,
};
use midenc_hir::{Context, dialects::builtin};
use module::build_ir::translate_module_as_component;
use wasmparser::{Payload, WasmFeatures};

#[cfg(feature = "std")]
pub use self::emit::wasm_to_wat;
pub use self::{config::*, emit::WatEmit, error::WasmError};

/// The output of the frontend Wasm translation stage
pub struct FrontendOutput {
    /// The IR component translated from the Wasm
    pub component: builtin::ComponentRef,
    /// Out-of-band payloads destined for the compiled package's sections.
    pub sections: PackageSections,
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
        translate_module_as_component(wasm, config, context)
    } else {
        translate_component(wasm, config, context)
    }
}

/// Rejects note storage schema metadata from a core Wasm module.
fn reject_core_module_note_storage_schema(wasm: &[u8]) -> WasmResult<()> {
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let payload = payload.map_err(|error| -> midenc_session::diagnostics::Report {
            WasmError::from(error).into()
        })?;
        if let Payload::CustomSection(section) = payload
            && section.name() == WASM_NOTE_STORAGE_SCHEMA_CUSTOM_SECTION_NAME
        {
            return Err(WasmError::Unsupported(
                "a core WebAssembly module contains a note storage schema that cannot be \
                 preserved; compile the note crate as a WebAssembly component"
                    .to_owned(),
            )
            .into());
        }
    }
    Ok(())
}

/// The set of core WebAssembly features which we need to or wish to support
pub(crate) fn supported_features() -> WasmFeatures {
    WasmFeatures::BULK_MEMORY
        | WasmFeatures::FLOATS
        | WasmFeatures::FUNCTION_REFERENCES
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::MUTABLE_GLOBAL
        | WasmFeatures::REFERENCE_TYPES
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A `#[note]` crate compiled as a raw core module keeps its schema section: the
    /// translation wraps the module as a component and propagates the captured sections.
    #[test]
    fn core_module_propagates_note_storage_schema_section() {
        let wat = format!(
            r#"(module (@custom "{WASM_NOTE_STORAGE_SCHEMA_CUSTOM_SECTION_NAME}" "schema"))"#
        );
        let wasm = wat::parse_str(wat).expect("core module WAT must parse");
        let context = Rc::new(Context::default());
        let output = translate(&wasm, &WasmTranslationConfig::default(), context)
            .expect("a core module with a schema section must translate");
        assert_eq!(
            output.sections.note_storage_schema.as_deref(),
            Some(b"schema".as_slice()),
            "the schema section must be propagated into the frontend output"
        );
    }
}
