use std::{cell::RefCell, rc::Rc};

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_hir::{
    dialects::builtin::{BuiltinOpBuilder, Function, FunctionRef, ModuleBuilder, WorldBuilder},
    AbiParam, CallConv, FunctionIdent, FunctionType, FxHashMap, Ident, Op, Signature, Symbol,
    SymbolName, SymbolNameComponent, SymbolPath, SymbolRef, SymbolTable, UnsafeIntrusiveEntityRef,
    ValueRef, Visibility,
};
use midenc_session::diagnostics::{DiagnosticsHandler, Severity};

use super::{
    function_builder_ext::{FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener},
    instance::ModuleArgument,
    ir_func_type,
    types::ModuleTypesBuilder,
    EntityIndex, FuncIndex, Module, ModuleTypes,
};
use crate::{
    component::lower_imports::generate_import_lowering_function,
    error::WasmResult,
    intrinsics::{
        intrinsics_conversion_result, is_miden_intrinsics_module, IntrinsicsConversionResult,
    },
    miden_abi::{
        is_miden_abi_module, miden_abi_function_type, recover_imported_masm_function_id,
        transform::transform_miden_abi_call,
    },
    translation_utils::sig_from_func_type,
};

/// Local or imported core Wasm module function
#[derive(Clone)]
pub struct CallableFunction {
    /// Module and function name parsed from the core Wasm module
    pub wasm_id: FunctionIdent,
    /// Defined IR function or None if it's an intrinsic that will be represented with an op
    pub function_ref: Option<FunctionRef>,
    /// Function signature parsed from the core Wasm module
    pub signature: Signature,
}

pub struct ModuleTranslationState<'a> {
    /// Imported and local functions
    functions: FxHashMap<FuncIndex, CallableFunction>,
    pub module_builder: &'a mut ModuleBuilder,
}

impl<'a> ModuleTranslationState<'a> {
    pub fn new(
        module: &Module,
        module_builder: &'a mut ModuleBuilder,
        world_builder: &'a mut WorldBuilder,
        mod_types: &ModuleTypesBuilder,
        module_args: FxHashMap<FunctionIdent, ModuleArgument>,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<Self> {
        // TODO: extract into `fn process_module_imports` after component translation is
        // implemented

        let mut functions = FxHashMap::default();
        for (index, func_type) in &module.functions {
            let wasm_func_type = mod_types[func_type.signature].clone();
            let ir_func_type = ir_func_type(&wasm_func_type, diagnostics)?;
            let func_name = module.func_name(index);
            let func_id = FunctionIdent {
                module: Ident::from(module.name().as_str()),
                function: Ident::from(func_name.as_str()),
            };
            let sig = sig_from_func_type(&ir_func_type, CallConv::SystemV, Visibility::Public);
            if module.is_imported_function(index) {
                assert!((index.as_u32() as usize) < module.num_imported_funcs);
                let import = &module.imports[index.as_u32() as usize];
                let wasm_import_func_id = FunctionIdent {
                    module: Ident::from(import.module.as_str()),
                    function: Ident::from(import.field.as_str()),
                };
                let import_func_id =
                    recover_imported_masm_function_id(import.module.as_str(), &import.field);
                let callable_function =
                    if is_miden_intrinsics_module(import_func_id.module.as_symbol()) {
                        if intrinsics_conversion_result(&import_func_id).is_operation() {
                            CallableFunction {
                                wasm_id: import_func_id,
                                function_ref: None,
                                signature: sig,
                            }
                        } else {
                            define_func_for_intrinsic(world_builder, sig, import_func_id)
                        }
                    } else if is_miden_abi_module(import_func_id.module.as_symbol()) {
                        define_func_for_miden_abi_trans(
                            world_builder,
                            module_builder,
                            func_id,
                            sig,
                            import_func_id,
                        )
                    } else if let Some(module_arg) = module_args.get(&wasm_import_func_id) {
                        match module_arg {
                            ModuleArgument::Function(function_ident) => {
                                todo!("core Wasm function import is not implemented yet");
                                //generate the internal function and call the import argument  function"
                            }
                            ModuleArgument::ComponentImport(signature) => {
                                generate_import_lowering_function(
                                    world_builder,
                                    module_builder,
                                    wasm_import_func_id,
                                    signature,
                                    func_id,
                                    sig,
                                    diagnostics,
                                )?
                            }
                            ModuleArgument::Table => {
                                todo!("implement the table import module arguments")
                            }
                        }
                    } else {
                        panic!("unexpected import {import:?}");
                    };
                functions.insert(index, callable_function);
            } else {
                let func_ref = module_builder
                    .define_function(func_id.function, sig.clone())
                    .expect("adding new function failed");
                let defined_function = CallableFunction {
                    wasm_id: func_id,
                    function_ref: Some(func_ref),
                    signature: sig.clone(),
                };
                functions.insert(index, defined_function);
            };
        }
        Ok(Self {
            functions,
            module_builder,
        })
    }

    /// Get the `FunctionIdent` that should be used to make a direct call to function
    /// `index`.
    pub(crate) fn get_direct_func(
        &mut self,
        index: FuncIndex,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<CallableFunction> {
        let defined_func = self.functions[&index].clone();
        Ok(defined_func)
    }
}

// TODO: move to abi_transform module
fn define_func_for_miden_abi_trans(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    synth_func_id: FunctionIdent,
    synth_func_sig: Signature,
    import_func_id: FunctionIdent,
) -> CallableFunction {
    let import_ft = miden_abi_function_type(
        import_func_id.module.as_symbol(),
        import_func_id.function.as_symbol(),
    );
    let import_sig = Signature::new(
        import_ft.params.into_iter().map(AbiParam::new),
        import_ft.results.into_iter().map(AbiParam::new),
    );
    let mut func_ref = module_builder
        .define_function(synth_func_id.function, synth_func_sig.clone())
        .expect("failed to create an import function");
    let mut func = func_ref.borrow_mut();
    let span = func.name().span;
    let context = func.as_operation().context_rc();
    let func = func.as_mut().downcast_mut::<Function>().unwrap();
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    let mut func_builder = FunctionBuilderExt::new(func, &mut op_builder);
    let entry_block = func_builder.current_block();
    func_builder.seal_block(entry_block); // Declare all predecessors known.
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    let import_module_ref = if let Some(found_module_ref) =
        world_builder.find_module(import_func_id.module.as_symbol())
    {
        found_module_ref
    } else {
        world_builder
            .declare_module(import_func_id.module)
            .expect("failed to create a module for imports")
    };
    let mut import_module_builder = ModuleBuilder::new(import_module_ref);
    let import_func_ref = import_module_builder
        .define_function(import_func_id.function, import_sig.clone())
        .expect("failed to create an import function");
    let results = transform_miden_abi_call(
        import_func_ref,
        import_func_id,
        args.as_slice(),
        &mut func_builder,
    );

    let exit_block = func_builder.create_block();
    func_builder.append_block_params_for_function_returns(exit_block);
    func_builder.br(exit_block, results, span);
    func_builder.seal_block(exit_block);
    func_builder.switch_to_block(exit_block);
    func_builder.ret(None, span).expect("failed ret");

    CallableFunction {
        wasm_id: synth_func_id,
        function_ref: Some(func_ref),
        signature: synth_func_sig,
    }
}

// TODO: move to intrinsics module
fn define_func_for_intrinsic(
    world_builder: &mut WorldBuilder,
    sig: Signature,
    func_id: FunctionIdent,
) -> CallableFunction {
    let import_module_ref =
        if let Some(found_module_ref) = world_builder.find_module(func_id.module.as_symbol()) {
            found_module_ref
        } else {
            world_builder
                .declare_module(func_id.module)
                .expect("failed to create a module for imports")
        };
    let mut import_module_builder = ModuleBuilder::new(import_module_ref);
    let import_func_ref = import_module_builder
        .define_function(func_id.function, sig.clone())
        .expect("failed to create an import function");
    CallableFunction {
        wasm_id: func_id,
        function_ref: Some(import_func_ref),
        signature: sig,
    }
}
