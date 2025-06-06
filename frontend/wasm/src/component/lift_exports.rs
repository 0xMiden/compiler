use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::{BuiltinOpBuilder, ComponentBuilder, ModuleBuilder},
    interner::Symbol,
    AbiParam, AddressSpace, CallConv, FunctionType, Ident, Op, PointerType, Signature, SmallVec,
    SourceSpan, SymbolPath, Type, ValueRange, ValueRef,
};
use midenc_session::{diagnostics::Severity, DiagnosticsHandler};

use super::flat::{flatten_function_type, needs_transformation};
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

    let export_func_ident =
        Ident::new(Symbol::intern(export_func_name.to_string()), SourceSpan::default());

    let core_export_module_path = core_export_func_path.without_leaf();
    let core_module_ref = component_builder
        .resolve_module(&core_export_module_path)
        .expect("failed to find the core module");

    let core_module_builder = ModuleBuilder::new(core_module_ref);
    let core_export_func_ref = core_module_builder
        .get_function(core_export_func_path.name().as_str())
        .expect("failed to find the core module function");
    let core_export_func_sig = core_export_func_ref.borrow().signature().clone();

    // TODO: where do want to check it? if at all
    // assert_core_wasm_signature_equivalence(&core_export_func_sig, &cross_ctx_export_sig);

    if needs_transformation(&cross_ctx_export_sig) {
        // Check if we have a pointer in results (canon lift case)
        let has_result_pointer = cross_ctx_export_sig
            .results()
            .iter()
            .any(|result| result.purpose == midenc_hir::ArgumentPurpose::StructReturn);

        // FIX: ugly
        if has_result_pointer {
            // TODO: 1. make it generic 2. do the same in import
            let new_sig = Signature {
                params: cross_ctx_export_sig.params,
                results: vec![
                    AbiParam::new(Type::Felt),
                    AbiParam::new(Type::Felt),
                    AbiParam::new(Type::Felt),
                    AbiParam::new(Type::Felt),
                ],
                cc: cross_ctx_export_sig.cc,
                visibility: cross_ctx_export_sig.visibility,
            };
            let export_func_ref = component_builder.define_function(export_func_ident, new_sig)?;

            let (span, context) = {
                let export_func = export_func_ref.borrow();
                (export_func.name().span, export_func.as_operation().context_rc())
            };
            let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
            let mut op_builder = midenc_hir::OpBuilder::new(context)
                .with_listener(SSABuilderListener::new(func_ctx));
            let mut fb = FunctionBuilderExt::new(export_func_ref, &mut op_builder);

            let entry_block = fb.current_block();
            fb.seal_block(entry_block);
            let args: Vec<ValueRef> = entry_block
                .borrow()
                .arguments()
                .iter()
                .copied()
                .map(|ba| ba as ValueRef)
                .collect();

            // Export lifting: The core function returns a pointer to the result
            // We need to:
            // 1. Call the core function which returns a pointer
            // 2. Load the data from that pointer
            // 3. Return it as individual values (tuple)

            let exec = fb.exec(core_export_func_ref, core_export_func_sig.clone(), args, span)?;

            let borrow = exec.borrow();
            let results = borrow.results().all();

            // The core function should return a single pointer (as i32)
            assert_eq!(results.len(), 1, "expected single pointer result");
            let result_ptr = results.into_iter().next().unwrap().borrow().as_value_ref();

            // Export lifting: The core function returns a pointer (i32) to the result

            let mut return_values = SmallVec::<[ValueRef; 4]>::new();
            let felt_ptr_type =
                Type::from(PointerType::new_with_address_space(Type::Felt, AddressSpace::Byte));
            // TODO: make it generic
            for idx in 0..4 {
                // Load from the pointer
                let src_offset = fb.i32(idx * 4, span); // felt is 4 bytes
                let src_addr = fb.add_unchecked(result_ptr, src_offset, span)?;
                let src_ptr = fb.inttoptr(src_addr, felt_ptr_type.clone(), span)?;
                let value = fb.load(src_ptr, span)?;
                return_values.push(value);
            }

            // Return the loaded values
            let exit_block = fb.create_block();
            fb.br(exit_block, return_values.clone(), span)?;
            fb.append_block_params_for_function_returns(exit_block);
            fb.seal_block(exit_block);
            fb.switch_to_block(exit_block);
            fb.ret(return_values, span)?;
        }
    } else {
        let export_func_ref =
            component_builder.define_function(export_func_ident, cross_ctx_export_sig.clone())?;

        let (span, context) = {
            let export_func = export_func_ref.borrow();
            (export_func.name().span, export_func.as_operation().context_rc())
        };
        let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
        let mut op_builder =
            midenc_hir::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
        let mut fb = FunctionBuilderExt::new(export_func_ref, &mut op_builder);

        let entry_block = fb.current_block();
        fb.seal_block(entry_block); // Declare all predecessors known.
        let args: Vec<ValueRef> = entry_block
            .borrow()
            .arguments()
            .iter()
            .copied()
            .map(|ba| ba as ValueRef)
            .collect();

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
    }

    Ok(())
}
