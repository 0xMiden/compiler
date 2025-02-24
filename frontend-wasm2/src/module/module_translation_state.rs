use std::{cell::RefCell, rc::Rc};

use midenc_dialect_hir::InstBuilder;
use midenc_hir::diagnostics::{DiagnosticsHandler, Severity};
use midenc_hir2::{
    dialects::builtin::{ComponentBuilder, Function, FunctionRef, ModuleBuilder},
    AbiParam, CallConv, FunctionIdent, FunctionType, FxHashMap, Ident, Op, Signature, Symbol,
    SymbolName, SymbolNameComponent, SymbolPath, SymbolRef, SymbolTable, UnsafeIntrusiveEntityRef,
    ValueRef, Visibility,
};

use super::{
    function_builder_ext::{FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener},
    instance::ModuleArgument,
    ir_func_type, EntityIndex, FuncIndex, Module, ModuleTypes,
};
use crate::{
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
        component_builder: &'a mut ComponentBuilder,
        mod_types: &ModuleTypes,
        module_args: Vec<ModuleArgument>,
        diagnostics: &DiagnosticsHandler,
    ) -> Self {
        // TODO: extract into `fn process_module_imports` after component translation is
        // implemented
        let mut function_import_subst = FxHashMap::default();
        if module.imports.len() == module_args.len() {
            for (import, arg) in module.imports.iter().zip(module_args) {
                match (import.index, arg) {
                    (EntityIndex::Function(func_idx), ModuleArgument::Function(func_id)) => {
                        // Substitutes the function import with concrete function exported from
                        // another module
                        function_import_subst.insert(func_idx, func_id);
                    }
                    (EntityIndex::Function(_), ModuleArgument::ComponentImport(_)) => {
                        // Do nothing, the local function id will be used
                    }
                    (EntityIndex::Function(_), module_arg) => {
                        panic!(
                            "Unexpected {module_arg:?} module argument for function import \
                             {import:?}"
                        )
                    }
                    (..) => (), // Do nothing, we interested only in function imports
                }
            }
        }
        let mut functions = FxHashMap::default();
        for (index, func_type) in &module.functions {
            let wasm_func_type = mod_types[func_type.signature].clone();
            let ir_func_type = ir_func_type(&wasm_func_type, diagnostics).unwrap();
            let func_name = module.func_name(index);
            let func_id = FunctionIdent {
                module: Ident::from(module.name().as_str()),
                function: Ident::from(func_name.as_str()),
            };
            let sig = sig_from_func_type(&ir_func_type, CallConv::SystemV, Visibility::Public);
            if let Some(subst) = function_import_subst.get(&index) {
                // functions.insert(index, (*subst, sig));
                todo!("define the import in some symbol table");
            } else if module.is_imported_function(index) {
                assert!((index.as_u32() as usize) < module.num_imported_funcs);
                let import = &module.imports[index.as_u32() as usize];
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
                            define_func_for_intrinsic(component_builder, sig, import_func_id)
                        }
                    } else if is_miden_abi_module(import_func_id.module.as_symbol()) {
                        define_func_for_miden_abi_trans(
                            component_builder,
                            module_builder,
                            func_id,
                            sig,
                            import_func_id,
                        )
                    } else {
                        todo!("no intrinsics and no abi transformation import");
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
        Self {
            functions,
            module_builder,
        }
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

fn define_func_for_miden_abi_trans(
    component_builder: &mut ComponentBuilder,
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
        midenc_hir2::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
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
        component_builder.find_module(import_func_id.module.as_symbol())
    {
        found_module_ref
    } else {
        component_builder
            .define_module(import_func_id.module)
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
    func_builder.ins().br(exit_block, results, span);
    func_builder.seal_block(exit_block);
    func_builder.switch_to_block(exit_block);
    func_builder.ins().ret(None, span).expect("failed ret");

    CallableFunction {
        wasm_id: synth_func_id,
        function_ref: Some(func_ref),
        signature: synth_func_sig,
    }
}

fn define_func_for_intrinsic(
    component_builder: &mut ComponentBuilder,
    sig: Signature,
    func_id: FunctionIdent,
) -> CallableFunction {
    let import_module_ref =
        if let Some(found_module_ref) = component_builder.find_module(func_id.module.as_symbol()) {
            found_module_ref
        } else {
            component_builder
                .define_module(func_id.module)
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
