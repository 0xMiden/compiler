use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::{BuiltinOpBuilder, ComponentBuilder, Function, ModuleBuilder},
    interner::Symbol,
    CallConv, FunctionType, Ident, Op, SourceSpan, SymbolPath, ValueRange, ValueRef,
};
use midenc_session::{diagnostics::Severity, DiagnosticsHandler};

use super::flat::{
    assert_core_wasm_signature_equivalence, flatten_function_type, needs_transformation,
};
use crate::{
    error::WasmResult,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

pub fn generate_export_lifting_function(
    component_builder: &mut ComponentBuilder,
    export_func_name: &str,
    export_func_ty: FunctionType,
    core_export_func_path: SymbolPath,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    let cross_ctx_export_sig = flatten_function_type(&export_func_ty, CallConv::CanonLift)
        .map_err(|e| {
            let message = format!(
                "Component export lifting generation. Signature for exported function {} requires \
                 flattening. Error: {}",
                core_export_func_path, e
            );
            diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
        })?;
    if needs_transformation(&cross_ctx_export_sig) {
        let message = format!(
            "Component export lifting generation. Signature for exported function {} requires \
             lifting. This is not yet supported",
            core_export_func_path
        );
        return Err(diagnostics.diagnostic(Severity::Error).with_message(message).into_report());
    }

    let export_func_ident =
        Ident::new(Symbol::intern(export_func_name.to_string()), SourceSpan::default());
    let mut export_func_ref =
        component_builder.define_function(export_func_ident, cross_ctx_export_sig.clone())?;

    let core_export_module_path = core_export_func_path.without_leaf();
    let core_module_ref = component_builder
        .resolve_module(&core_export_module_path)
        .expect("failed to find the core module");

    let core_module_builder = ModuleBuilder::new(core_module_ref);
    let core_export_func_ref = core_module_builder
        .get_function(core_export_func_path.name().as_str())
        .expect("failed to find the core module function");
    let core_export_func_sig = core_export_func_ref.borrow().signature().clone();
    assert_core_wasm_signature_equivalence(&core_export_func_sig, &cross_ctx_export_sig);

    let mut export_func = export_func_ref.borrow_mut();
    let context = export_func.as_operation().context_rc();
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    let span = export_func.name().span;
    let mut fb = FunctionBuilderExt::new(
        export_func.as_mut().downcast_mut::<Function>().unwrap(),
        &mut op_builder,
    );

    let entry_block = fb.current_block();
    fb.seal_block(entry_block); // Declare all predecessors known.
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    // NOTE: handle CC lifting/lowering for non-scalar types
    // see https://github.com/0xPolygonMiden/compiler/issues/369

    let exec = fb
        .exec(core_export_func_ref, core_export_func_sig, args, span)
        .expect("failed to build an exec op");

    let borrow = exec.borrow();
    let results = ValueRange::<2>::from(borrow.results().all());
    assert!(results.len() <= 1, "expected a single result or none");

    let exit_block = fb.create_block();
    fb.br(exit_block, vec![], span).expect("failed br");
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let returning_onty_first = results.iter().take(1);
    fb.ret(returning_onty_first, span).expect("failed ret");

    Ok(())
}
