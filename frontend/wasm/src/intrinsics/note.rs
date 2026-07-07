//! Note intrinsics conversion module for WebAssembly to Miden IR.
//!
//! These intrinsics expose compile-time facts about the note script defined by the current
//! project. Unlike the other intrinsic kinds they cannot be lowered from the intrinsic name
//! alone: `script_root` receives a Rust function reference to the note-script entrypoint, which
//! only has meaning relative to the module context — the function table the reference indexes
//! and the frontend metadata emitted by the SDK macros. They are therefore converted inline at
//! call sites, where that context is available.

use midenc_dialect_hir::HirOpBuilder;
use midenc_frontend_wasm_metadata::FrontendMetadata;
use midenc_hir::{
    Builder, Op, SmallVec, SourceSpan, SymbolNameComponent, ValueRef,
    diagnostics::{DiagnosticsHandler, Report, Severity},
    dialects::builtin::{FunctionTableEntry, FunctionTableRef, attributes::UnitAttr},
    interner::{Symbol, symbols},
};

use crate::{
    error::WasmResult,
    miden_abi::transform::store_results_to_pointer,
    module::{
        Module,
        function_builder_ext::FunctionBuilderExt,
        module_translation_state::ModuleTranslationState,
        types::{GlobalInit, TableIndex},
    },
};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::Note),
];

/// Converts a call to a note intrinsic.
///
/// `args` are the call-site arguments; the returned values are the results the call site pushes.
pub(crate) fn convert_note_intrinsics_call<B: ?Sized + Builder>(
    function: Symbol,
    args: &[ValueRef],
    module: &Module,
    module_state: &mut ModuleTranslationState,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    match function.as_str() {
        "script_root" => {
            assert_eq!(
                args.len(),
                2,
                "{function} takes exactly two arguments (the entrypoint function reference and \
                 the result pointer)"
            );

            if !matches!(
                module_state.frontend_metadata(),
                Some(FrontendMetadata::NoteScript { .. })
            ) {
                return Err(Report::msg(
                    "`get_entrypoint_root()` requires a `#[note_script]` entrypoint in the \
                     current project",
                ));
            }

            // A Rust function reference compiles to a constant index into the module's sole
            // `funcref` table (table 0 in every rustc/wasm-ld layout) — the same table
            // `call_indirect` dispatches through. Resolve it back to the table slot it denotes.
            let Some(slot) = constant_table_slot(args[0], module) else {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message(
                        "invalid `get_entrypoint_root()` call: the entrypoint function reference \
                         is not statically resolvable",
                    )
                    .with_primary_label(
                        span,
                        "this call requires a compile-time constant function reference",
                    )
                    .with_help(
                        "the argument must be a direct reference to the `#[note_script]` \
                         entrypoint, not a value computed at runtime",
                    )
                    .into_report());
            };

            let table_index = TableIndex::from_u32(0);
            let table_slot_function =
                module_state.resolve_table_slot(table_index, slot, module, diagnostics)?;
            if table_slot_function.is_none() {
                return Err(diagnostics
                    .diagnostic(Severity::Error)
                    .with_message(format!(
                        "invalid `get_entrypoint_root()` call: the entrypoint function reference \
                         does not resolve to a function (function table slot {slot} is not \
                         initialized with one)",
                    ))
                    .with_primary_label(span, "this function reference cannot be resolved")
                    .with_help(
                        "the argument must be a direct reference to the `#[note_script]` \
                         entrypoint",
                    )
                    .into_report());
            }

            // Force the table to be lowered and mark the slot the reference denotes. The
            // referenced function is only a placeholder — export lifting repoints the marked
            // entry at the lifted note-script export, whose MAST root is the note script root
            // the transaction kernel observes — and codegen rejects any marked entry whose
            // callee does not carry the `note_script` attribute. The root itself is
            // materialized by `hir.function_table_root`, which resolves the marked entry to a
            // `procref` computed at assembly time.
            let table_ref = module_state.get_or_build_table(table_index, module, diagnostics)?;
            mark_note_script_root_slot(table_ref, slot);

            let op = builder.function_table_root(table_ref, slot, span)?;
            let results: SmallVec<[ValueRef; 4]> = {
                let borrow = op.borrow();
                borrow.results().iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
            };
            store_results_to_pointer(&results, args[1], builder)?;

            Ok(SmallVec::new())
        }
        unknown => panic!("unknown note intrinsic: {unknown}"),
    }
}

/// Resolves `value` to the constant function-table slot it denotes, if it can be computed at
/// compile time.
///
/// A Rust function reference reaches the intrinsic either as a plain constant or — in
/// position-independent modules — as `<table base> + <slot offset>`, where the table base is
/// read from an immutable, link-resolved global (`GOT.data.internal.__table_base`); both forms
/// fold to a constant here.
fn constant_table_slot(value: ValueRef, module: &Module) -> Option<u32> {
    fold_to_constant(value, module).and_then(|folded| u32::try_from(folded).ok())
}

/// Folds `value` to a compile-time constant, looking through additions and reads of immutable,
/// constant-initialized globals.
fn fold_to_constant(value: ValueRef, module: &Module) -> Option<i64> {
    let defining_op = value.borrow().get_defining_op()?;
    let defining_op = defining_op.borrow();

    if let Some(constant) = defining_op.downcast_ref::<midenc_dialect_arith::Constant>() {
        return constant.get_value().as_u32().map(i64::from);
    }
    if let Some(add) = defining_op.downcast_ref::<midenc_dialect_arith::Add>() {
        let lhs = fold_to_constant(add.lhs().as_value_ref(), module)?;
        let rhs = fold_to_constant(add.rhs().as_value_ref(), module)?;
        return lhs.checked_add(rhs);
    }
    // Signedness reinterpretations inserted between operands are value-preserving here: table
    // slots and bases are small non-negative integers
    if let Some(bitcast) = defining_op.downcast_ref::<midenc_dialect_hir::Bitcast>() {
        return fold_to_constant(bitcast.operand().as_value_ref(), module);
    }
    if let Some(load) = defining_op.downcast_ref::<midenc_dialect_hir::Load>() {
        return fold_immutable_global_read(load.addr().as_value_ref(), module);
    }
    None
}

/// Folds a load whose address is a (bitcast) `builtin.global_symbol` of an immutable,
/// constant-initialized Wasm global — the shape `global.get` translates to — to the global's
/// initializer value.
fn fold_immutable_global_read(addr: ValueRef, module: &Module) -> Option<i64> {
    use midenc_hir::dialects::builtin::GlobalSymbol;

    // `load_global` reifies the address as `builtin.global_symbol` bitcast to the pointee type
    let addr_op = addr.borrow().get_defining_op()?;
    let addr_op = addr_op.borrow();
    let symbol_value =
        addr_op.downcast_ref::<midenc_dialect_hir::Bitcast>()?.operand().as_value_ref();
    let symbol_op = symbol_value.borrow().get_defining_op()?;
    let symbol_op = symbol_op.borrow();
    let global_symbol = symbol_op.downcast_ref::<GlobalSymbol>()?;
    if *global_symbol.get_offset() != 0 {
        return None;
    }
    let global_name = global_symbol.get_symbol().path().name();

    let (global_index, global) = module
        .globals
        .iter()
        .find(|(index, _)| module.global_name(*index) == global_name.as_ref())?;
    if global.mutability {
        return None;
    }
    let defined_index = module.defined_global_index(global_index)?;
    match module.global_initializers[defined_index] {
        GlobalInit::I32Const(value) => Some(i64::from(value)),
        GlobalInit::I64Const(value) => Some(value),
        _ => None,
    }
}

/// Marks the [FunctionTableEntry] initializing `slot` as the slot holding the note script root.
fn mark_note_script_root_slot(table_ref: FunctionTableRef, slot: u32) {
    // Collect first: iterating a block body holds a borrow of each visited operation
    let entries: SmallVec<[midenc_hir::OperationRef; 4]> = {
        let table = table_ref.borrow();
        let entries = table.entries();
        let block = entries.entry();
        block.body().iter().map(|op| op.as_operation_ref()).collect()
    };
    for mut op_ref in entries {
        let mut op = op_ref.borrow_mut();
        let Some(entry) = op.downcast_mut::<FunctionTableEntry>() else {
            continue;
        };
        if *entry.get_index() != slot {
            continue;
        }
        let context = entry.as_operation().context_rc();
        let attr = context.create_attribute::<UnitAttr, _>(());
        entry
            .as_operation_mut()
            .set_attribute(FunctionTableEntry::NOTE_SCRIPT_ROOT_SLOT_ATTR, attr);
    }
}
