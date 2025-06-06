//! lowering the imports into the Miden ABI for the cross-context calls

use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    diagnostics::WrapErr,
    dialects::builtin::{
        BuiltinOpBuilder, ComponentBuilder, ComponentId, ModuleBuilder, WorldBuilder,
    },
    AbiParam, AddressSpace, CallConv, FunctionType, Op, PointerType, Signature, SymbolPath, Type,
    ValueRef,
};

use super::flat::{flatten_function_type, needs_transformation};
use crate::{
    callable::CallableFunction,
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

    // assert_core_wasm_signature_equivalence(&core_func_sig, &import_lowered_sig);

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

    if needs_transformation(&import_lowered_sig) {
        // When transformation is needed, the import function's results are passed via a pointer parameter
        // This happens when the result type would flatten to more than 1 value

        // The import function should have the lifted signature (returns tuple)
        // not the lowered signature with pointer parameter
        let import_func_sig = flatten_function_type(import_func_ty, CallConv::CanonLower)
            .wrap_err_with(|| {
                format!("failed to flatten import function signature for '{}'", import_func_path)
            })?;

        // TODO: make it generic
        let new_import_func_sig = Signature {
            params: import_func_sig.params[..import_func_sig.params.len() - 1].to_vec(), // without
            // the last arg - pointer
            results: vec![
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
            ],
            cc: import_func_sig.cc,
            visibility: import_func_sig.visibility,
        };
        let import_func_ref = component_builder
            .define_function(import_func_path.name().into(), new_import_func_sig.clone())
            .expect("failed to define the import function");

        // Check if we have a pointer in params (result via out-pointer)
        let param_has_pointer = import_lowered_sig
            .params()
            .iter()
            .any(|param| param.purpose == midenc_hir::ArgumentPurpose::StructReturn);

        // FIX: ugly
        if param_has_pointer {
            // Import lowering: The lowered function takes a pointer as the last parameter
            // where results should be stored. The import function returns a pointer to the result.
            // We need to:
            // 1. Call the import function (it returns a tuple to the flattened result)
            // 2. Store the data from the tuple to the output pointer

            // Get the pointer argument (last argument) where we need to store results
            let output_ptr = args.last().expect("expected pointer argument");
            let args_without_ptr: Vec<_> = args[..args.len() - 1].to_vec();

            // Call the import function - it will return a tuple to the flattened result struct
            let call = fb.call(import_func_ref, new_import_func_sig, args_without_ptr, span)?;

            let borrow = call.borrow();
            let results_storage = borrow.as_ref().results();
            let results: Vec<ValueRef> =
                results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

            // TODO: make it generic
            // The result should be 4 felts
            assert_eq!(results.len(), 4, "expected 4 felts");

            let felt_type = Type::Felt;
            let felt_ptr_type = Type::from(PointerType::new_with_address_space(
                felt_type.clone(),
                AddressSpace::Byte,
            ));

            for (idx, value) in results.into_iter().enumerate() {
                // Store to destination pointer
                // TODO: make it generic
                let dst_offset = fb.i32((idx * 4) as i32, span);
                let dst_addr = fb.add_unchecked(*output_ptr, dst_offset, span)?;
                let dst_ptr = fb.inttoptr(dst_addr, felt_ptr_type.clone(), span)?;
                fb.store(dst_ptr, value, span)?;
            }

            let exit_block = fb.create_block();
            fb.br(exit_block, [], span)?;
            fb.seal_block(exit_block);
            fb.switch_to_block(exit_block);
            // TODO: return according to the function sig - core_func_sig
            fb.ret([], span)?;

            Ok(CallableFunction::Function {
                wasm_id: core_func_path,
                function_ref: core_func_ref,
                signature: core_func_sig,
            })
        } else {
            panic!("no pointer")
        }
    } else {
        let import_func_sig = flatten_function_type(import_func_ty, CallConv::CanonLift)
            .wrap_err_with(|| {
                format!("failed to flatten import function signature for '{}'", import_func_path)
            })?;
        let import_func_ref = component_builder
            .define_function(import_func_path.name().into(), import_func_sig.clone())
            .expect("failed to define the import function");

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
}
