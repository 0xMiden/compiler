//! lowering the imports into the Miden ABI for the cross-context calls

use std::{cell::RefCell, rc::Rc, str::FromStr};

use midenc_dialect_hir::InstBuilder;
use midenc_hir2::{
    dialects::builtin::{ComponentBuilder, ComponentId, Function, ModuleBuilder, WorldBuilder},
    CallConv, FunctionIdent, FunctionType, Op, Signature, ValueRef,
};
use midenc_session::{diagnostics::Severity, DiagnosticsHandler};

use super::flat::{
    assert_core_wasm_signature_equivalence, flatten_function_type, needs_transformation,
};
use crate::{
    error::WasmResult,
    module::{
        function_builder_ext::{FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener},
        module_translation_state::CallableFunction,
    },
};

/// Generates the lowering function (cross-context Miden ABI -> Wasm CABI) for the given import function.
pub fn generate_import_lowering_function(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    import_func_id: FunctionIdent,
    import_func_ty: &FunctionType,
    core_func_id: FunctionIdent,
    core_func_sig: Signature,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<CallableFunction> {
    let import_lowered_sig =
        flatten_function_type(import_func_ty, CallConv::CanonLower).map_err(|e| {
            let message = format!(
                "Component import lowering generation. Signature for imported function {} \
                 requires flattening. Error: {}",
                import_func_id.function, e
            );
            diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
        })?;

    if needs_transformation(&import_lowered_sig) {
        let message = format!(
            "Component import lowering generation. Signature for imported function {} requires \
             lowering. This is not supported yet.",
            import_func_id
        );
        return Err(diagnostics.diagnostic(Severity::Error).with_message(message).into_report());
    }
    assert_core_wasm_signature_equivalence(&core_func_sig, &import_lowered_sig);

    let mut core_func_ref = module_builder
        .define_function(core_func_id.function, core_func_sig.clone())
        .expect("failed to define the core function");

    let mut core_func = core_func_ref.borrow_mut();
    let context = core_func.as_operation().context_rc();
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir2::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    let span = core_func.name().span;
    let mut fb = FunctionBuilderExt::new(
        core_func.as_mut().downcast_mut::<Function>().unwrap(),
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

    let id = ComponentId::from_str(import_func_id.module.as_str())
        .expect("failed to parse component id");
    let component_ref = if let Some(component_ref) = world_builder.find_component(&id) {
        component_ref
    } else {
        world_builder
            .define_component(id.namespace.into(), id.name.into(), id.version)
            .expect("failed to define the component")
    };

    let mut component_builder = ComponentBuilder::new(component_ref);
    let import_func_ref = component_builder
        .define_function(import_func_id.function, core_func_sig.clone())
        .expect("failed to define the import function");

    // NOTE: handle CC lifting/lowering for non-scalar types
    // see https://github.com/0xPolygonMiden/compiler/issues/369

    let call = fb
        .ins()
        .call(import_func_ref, core_func_sig.clone(), args.to_vec(), span)
        .expect("failed to build an exec op");

    let borrow = call.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
    assert!(results.len() <= 1, "expected a single result or none");

    let exit_block = fb.create_block();
    fb.ins().br(exit_block, vec![], span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let returning = results.first().cloned();
    fb.ins().ret(returning, span).expect("failed ret");

    Ok(CallableFunction {
        wasm_id: core_func_id,
        function_ref: Some(core_func_ref),
        signature: core_func_sig,
    })
}
