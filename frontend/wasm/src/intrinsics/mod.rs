mod intrinsic;

pub use self::intrinsic::*;

pub mod advice;
pub mod crypto;
pub mod debug;
pub mod felt;
pub mod mem;
pub mod note;

use midenc_frontend_wasm_metadata::FrontendMetadata;
use midenc_hir::{Builder, SmallVec, SourceSpan, ValueRef, dialects::builtin::FunctionRef};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

/// Convert a call to a Miden intrinsic function into instruction(s)
pub fn convert_intrinsics_call<B: ?Sized + Builder>(
    intrinsic: Intrinsic,
    function_ref: Option<FunctionRef>,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    match intrinsic {
        Intrinsic::Debug(function) => {
            debug::convert_debug_intrinsics(function, function_ref, args, builder, span)
        }
        Intrinsic::Mem(function) => {
            mem::convert_mem_intrinsics(function, function_ref, args, builder, span)
        }
        Intrinsic::Felt(function) => {
            felt::convert_felt_intrinsics(function, function_ref, args, builder, span)
        }
        Intrinsic::Crypto(function) => {
            crypto::convert_crypto_intrinsics(function, function_ref, args, builder, span)
        }
        Intrinsic::Advice(function) => {
            advice::convert_advice_intrinsics(function, function_ref, args, builder, span)
        }
        // Module-context stubs are never registered as inline or import callables, so calls to
        // them always remain ordinary calls to the stub function
        Intrinsic::Note(function) => {
            panic!(
                "note intrinsic '{function}' is a module-context stub and must be lowered via \
                 `convert_module_context_stub_call`"
            )
        }
    }
}

/// Synthesizes the body of a linker stub whose lowering requires module-level context.
///
/// This is the conversion entry point for intrinsics classified as
/// [`IntrinsicsConversionResult::ModuleContextStub`]: `stub_function_ref` is the recognized stub
/// function itself, `args` are its entry-block arguments, and the returned values become its
/// results.
pub fn convert_module_context_stub_call<B: ?Sized + Builder>(
    intrinsic: Intrinsic,
    stub_function_ref: FunctionRef,
    args: &[ValueRef],
    frontend_metadata: Option<&FrontendMetadata>,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    match intrinsic {
        Intrinsic::Note(function) => note::convert_note_intrinsics_stub(
            function,
            stub_function_ref,
            args,
            frontend_metadata,
            builder,
            span,
        ),
        other => panic!(
            "intrinsic '{}' is not lowered as a module-context stub",
            other.into_symbol_path()
        ),
    }
}
