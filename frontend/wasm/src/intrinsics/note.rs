//! Note intrinsics conversion module for WebAssembly to Miden IR.
//!
//! These intrinsics expose compile-time facts about the note script defined by the current
//! project. Unlike the other intrinsic kinds they cannot be lowered from the intrinsic name
//! alone: they reference the project's `#[note_script]` entrypoint, which is identified by the
//! frontend metadata emitted by the SDK macros. They are therefore lowered exclusively through
//! the linker-stub path ([`IntrinsicsConversionResult::ModuleContextStub`]), where the parsed
//! module's metadata is available.
//!
//! [`IntrinsicsConversionResult::ModuleContextStub`]: super::IntrinsicsConversionResult::ModuleContextStub

use midenc_dialect_hir::{HirOpBuilder, ProcedureRoot};
use midenc_frontend_wasm_metadata::FrontendMetadata;
use midenc_hir::{
    Builder, Op, SourceSpan, SymbolNameComponent, ValueRef,
    diagnostics::Report,
    dialects::builtin::{FunctionRef, attributes::UnitAttr},
    interner::{Symbol, symbols},
};

use crate::{
    error::WasmResult, miden_abi::transform::store_results_to_pointer,
    module::function_builder_ext::FunctionBuilderExt,
};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::Note),
];

/// Core-module function name of the `script_root` linker stub.
///
/// This is the exported symbol name of the SDK stub the `get_entrypoint_root()` binding calls;
/// export lifting uses it to locate the stub and repoint its `hir.procedure_root` op at the
/// lifted note-script export.
pub(crate) const SCRIPT_ROOT_STUB_NAME: &str = "intrinsics::note::script_root";

/// Synthesizes the body of a note intrinsic linker stub.
///
/// `stub_function_ref` is the stub function whose body is being synthesized; `args` are its
/// entry-block arguments and the returned values are its results.
pub(crate) fn convert_note_intrinsics_stub<B: ?Sized + Builder>(
    function: Symbol,
    stub_function_ref: FunctionRef,
    args: &[ValueRef],
    metadata: Option<&FrontendMetadata>,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    match function.as_str() {
        "script_root" => {
            if args.len() != 1 {
                return Err(Report::msg(format!(
                    "invalid `{SCRIPT_ROOT_STUB_NAME}` stub: expected exactly one parameter (the \
                     result pointer), but the stub declares {}",
                    args.len()
                )));
            }

            if !matches!(metadata, Some(FrontendMetadata::NoteScript { .. })) {
                return Err(Report::msg(
                    "`get_entrypoint_root()` requires a `#[note_script]` entrypoint in the \
                     current project",
                ));
            }

            // The note-script export wrapper — the procedure whose MAST root is the note script
            // root observed by the transaction kernel — does not exist yet: component exports
            // are lifted after core modules are translated. Build the op against the stub itself
            // as a placeholder and mark it with the intent attribute; export lifting repoints
            // marked ops at the lifted note-script export, and codegen rejects any marked op
            // whose callee does not carry the `note_script` attribute.
            let op = builder.procedure_root(stub_function_ref, span)?;
            {
                let context = stub_function_ref.borrow().as_operation().context_rc();
                let attr = context.create_attribute::<UnitAttr, _>(());
                let mut op = op;
                op.borrow_mut()
                    .as_operation_mut()
                    .set_attribute(ProcedureRoot::NOTE_SCRIPT_ROOT_ATTR, attr);
            }
            let results: Vec<ValueRef> = {
                let borrow = op.borrow();
                borrow.results().iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
            };
            store_results_to_pointer(&results, args[0], builder)?;

            Ok(Vec::new())
        }
        // Every function under the note module routes here (the module as a whole is classified
        // as a module-context stub), so an unknown name is malformed input, not a compiler bug.
        unknown => {
            Err(Report::msg(format!("unknown note intrinsic: 'intrinsics::note::{unknown}'")))
        }
    }
}
