//! lowering the imports into the Miden ABI for the cross-context calls

use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    diagnostics::WrapErr,
    dialects::builtin::{
        BuiltinOpBuilder, ComponentBuilder, ComponentId, ModuleBuilder, WorldBuilder,
    },
    CallConv, FunctionType, Op, Report, Signature, SymbolPath, ValueRef,
};

use super::flat::{flatten_function_type, needs_transformation};
use crate::{
    callable::CallableFunction,
    component::flat::assert_core_wasm_signature_equivalence,
    error::WasmResult,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

/// Generates the lowering function (cross-context Miden ABI -> Wasm CABI) for the given import function.
pub fn generate_import_lowering_function(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    import_func_path: SymbolPath,
    import_func_ty: &FunctionType,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
) -> WasmResult<CallableFunction> {
    let import_lowered_sig = flatten_function_type(import_func_ty, CallConv::CanonLower)
        .wrap_err_with(|| {
            format!(
                "failed to generate component import lowering: signature of '{import_func_path}' \
                 requires flattening"
            )
        })?;

    dbg!(&import_func_path.to_string());
    dbg!(&import_func_ty.to_string());
    dbg!(&import_lowered_sig.to_string());

    // TODO: remove after passing the pointee via the stack/advice provider
    if import_func_path.to_string() == "miden:cross-ctx-account-word/foo@1.0.0/process-word"
        || import_func_path.to_string()
            == "miden:cross-ctx-account-word/foo@1.0.0/process-another-word"
    {
        assert_eq!(
            import_func_ty.to_string(),
        "extern \"canon-lower\" fn(struct {struct {struct {felt}, struct {felt}, struct {\n    felt}, struct {felt}}}) -> struct {struct {struct {felt}, struct {\n    felt}, struct {felt}, struct {felt}}}".to_string(),
            "{} function is expected to have 'right' component-level type signature but found 'left'",
            import_func_path
        );
    } else {
        if needs_transformation(&import_lowered_sig) {
            return Err(Report::msg(format!(
                "Component import lowering generation. Signature for imported function \
                 '{import_func_path}' requires lowering. This is not supported yet.",
            )));
        }
        assert_core_wasm_signature_equivalence(&core_func_sig, &import_lowered_sig);
    }

    let core_func_ref = module_builder
        .define_function(core_func_path.name().into(), core_func_sig.clone())
        .expect("failed to define the core function");

    let (span, context) = {
        let core_func = core_func_ref.borrow();
        (core_func.name().span, core_func.as_operation().context_rc())
    };
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    let mut fb = FunctionBuilderExt::new(core_func_ref, &mut op_builder);

    let entry_block = fb.current_block();
    fb.seal_block(entry_block); // Declare all predecessors known.
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    let id = ComponentId::try_from(&import_func_path)
        .wrap_err("path does not start with a valid component id")?;
    let component_ref = if let Some(component_ref) = world_builder.find_component(&id) {
        component_ref
    } else {
        world_builder
            .define_component(id.namespace.into(), id.name.into(), id.version)
            .expect("failed to define the component")
    };

    let mut component_builder = ComponentBuilder::new(component_ref);
    let import_func_ref = component_builder
        .define_function(import_func_path.name().into(), core_func_sig.clone())
        .expect("failed to define the import function");

    // NOTE: handle CC lifting/lowering for non-scalar types
    // see https://github.com/0xMiden/compiler/issues/369

    let call = fb
        .call(import_func_ref, core_func_sig.clone(), args.to_vec(), span)
        .expect("failed to build an exec op");

    let borrow = call.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
    assert!(results.len() <= 1, "expected a single result or none");

    let exit_block = fb.create_block();
    fb.br(exit_block, vec![], span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let returning = results.first().cloned();
    fb.ret(returning, span).expect("failed ret");

    Ok(CallableFunction::Function {
        wasm_id: core_func_path,
        function_ref: core_func_ref,
        signature: core_func_sig,
    })
}
