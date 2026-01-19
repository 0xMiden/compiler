use midenc_hir::{
    AbiParam, CallConv, FunctionType, FxHashMap, Signature, SymbolNameComponent, SymbolPath,
    Visibility,
    diagnostics::WrapErr,
    dialects::builtin::{FunctionRef, ModuleBuilder, WorldBuilder},
    interner::Symbol,
    smallvec,
};
use midenc_session::diagnostics::{DiagnosticsHandler, Severity};

use super::{FuncIndex, Module, instance::ModuleArgument, ir_func_type, types::ModuleTypesBuilder};
use crate::{
    callable::CallableFunction, component::lower_imports::generate_import_lowering_function,
    error::WasmResult, intrinsics::Intrinsic, miden_abi::miden_abi_function_type,
    translation_utils::sig_from_func_type,
};

pub struct ModuleTranslationState<'a> {
    /// Imported and local functions
    functions: FxHashMap<FuncIndex, CallableFunction>,
    pub module_builder: &'a mut ModuleBuilder,
    pub world_builder: &'a mut WorldBuilder,
}

impl<'a> ModuleTranslationState<'a> {
    /// Create a new `ModuleTranslationState` for the core Wasm module translation
    ///
    /// Parameters:
    /// `module` - the core Wasm module
    /// `module_builder` - the Miden IR Module builder
    /// `world_builder` - the Miden IR World builder
    /// `mod_types` - the Miden IR module types builder
    /// `module_args` - the module instantiation arguments, i.e. entities to "fill" module imports
    pub fn new(
        module: &Module,
        module_builder: &'a mut ModuleBuilder,
        world_builder: &'a mut WorldBuilder,
        mod_types: &ModuleTypesBuilder,
        module_args: FxHashMap<SymbolPath, ModuleArgument>,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<Self> {
        let mut functions = FxHashMap::default();
        for (index, func_type) in &module.functions {
            let wasm_func_type = mod_types[func_type.signature].clone();
            let ir_func_type = ir_func_type(&wasm_func_type, diagnostics)?;
            let func_name = module.func_name(index);
            let path = SymbolPath {
                path: smallvec![
                    SymbolNameComponent::Root,
                    SymbolNameComponent::Component(module.name().as_symbol()),
                    SymbolNameComponent::Leaf(func_name)
                ],
            };
            let visibility = if module.is_exported(index.into()) {
                Visibility::Public
            } else {
                Visibility::Private
            };
            let sig = sig_from_func_type(&ir_func_type, CallConv::SystemV, visibility);
            if module.is_imported_function(index) {
                assert!((index.as_u32() as usize) < module.num_imported_funcs);
                let import = &module.imports[index.as_u32() as usize];
                let func = process_import(
                    module_builder,
                    world_builder,
                    &module_args,
                    path,
                    sig,
                    import,
                    diagnostics,
                )?;
                functions.insert(index, func);
            } else {
                let function_ref = module_builder
                    .define_function(path.name().into(), sig.clone())
                    .map_err(|e| {
                        diagnostics
                            .diagnostic(Severity::Error)
                            .with_message(format!(
                                "Failed to add new function '{}' to module: {e:?}",
                                path.name()
                            ))
                            .into_report()
                    })?;
                let defined_function = CallableFunction::Function {
                    wasm_id: path,
                    function_ref,
                    signature: sig.clone(),
                };
                functions.insert(index, defined_function);
            };
        }
        Ok(Self {
            functions,
            module_builder,
            world_builder,
        })
    }

    /// Get the `CallableFunction` that should be used to make a direct call to function `index`.
    pub(crate) fn get_direct_func(&mut self, index: FuncIndex) -> WasmResult<CallableFunction> {
        let defined_func = self.functions[&index].clone();
        Ok(defined_func)
    }

    /// Register a linker stub function as an intrinsic so that calls to it will be inlined.
    ///
    /// This updates the function's entry in the functions map from `CallableFunction::Function`
    /// to either `CallableFunction::Instruction` (for inline operations) or
    /// `CallableFunction::Intrinsic` (for MASM function calls).
    ///
    /// Returns the FunctionRef if the stub was registered (so it can be removed from the module),
    /// or None if the function wasn't found or isn't a valid intrinsic stub.
    pub(crate) fn register_linker_stub(
        &mut self,
        func_index: FuncIndex,
        intrinsic: Intrinsic,
    ) -> WasmResult<Option<FunctionRef>> {
        let Some(callable) = self.functions.get(&func_index) else {
            return Ok(None);
        };

        let CallableFunction::Function {
            function_ref,
            signature,
            ..
        } = callable
        else {
            return Ok(None);
        };

        let function_ref = *function_ref;
        let signature = signature.clone();

        // Determine if this intrinsic is inlined as an op or needs a function call
        let Some(conv) = intrinsic.conversion_result() else {
            return Ok(None);
        };

        if conv.is_function() {
            // Create import function reference for the intrinsic
            let import_path = intrinsic.into_symbol_path();
            let import_ft: FunctionType = intrinsic
                .function_type()
                .unwrap_or_else(|| miden_abi_function_type(&import_path));
            let import_sig = Signature::new(
                import_ft.params.into_iter().map(AbiParam::new),
                import_ft.results.into_iter().map(AbiParam::new),
            );

            let import_module_ref = self
                .world_builder
                .declare_module_tree(&import_path.without_leaf())
                .wrap_err("failed to create module for intrinsic imports")?;
            let mut import_module_builder = ModuleBuilder::new(import_module_ref);
            let intrinsic_func_ref = import_module_builder
                .define_function(import_path.name().into(), import_sig)
                .wrap_err("failed to create intrinsic function ref")?;

            self.functions.insert(
                func_index,
                CallableFunction::Intrinsic {
                    intrinsic,
                    function_ref: intrinsic_func_ref,
                    signature,
                },
            );
        } else {
            // Inline as an operation
            self.functions.insert(
                func_index,
                CallableFunction::Instruction {
                    intrinsic,
                    signature,
                },
            );
        }

        Ok(Some(function_ref))
    }
}

/// Returns [`CallableFunction`] translated from the core Wasm module import
fn process_import(
    module_builder: &mut ModuleBuilder,
    world_builder: &mut WorldBuilder,
    module_args: &FxHashMap<SymbolPath, ModuleArgument>,
    core_func_id: SymbolPath,
    core_func_sig: Signature,
    import: &super::ModuleImport,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<CallableFunction> {
    let import_path = SymbolPath {
        path: smallvec![
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(Symbol::intern(&import.module)),
            SymbolNameComponent::Leaf(Symbol::intern(&import.field))
        ],
    };
    let Some(module_arg) = module_args.get(&import_path) else {
        crate::unsupported_diag!(diagnostics, "unexpected import '{import_path:?}'");
    };
    process_module_arg(
        module_builder,
        world_builder,
        core_func_id,
        core_func_sig,
        import_path,
        module_arg,
    )
}

fn process_module_arg(
    module_builder: &mut ModuleBuilder,
    world_builder: &mut WorldBuilder,
    path: SymbolPath,
    sig: Signature,
    wasm_import_path: SymbolPath,
    module_arg: &ModuleArgument,
) -> WasmResult<CallableFunction> {
    Ok(match module_arg {
        ModuleArgument::Function(_) => {
            todo!("core Wasm function import is not implemented yet");
            //generate the internal function and call the import argument  function"
        }
        ModuleArgument::ComponentImport(signature) => generate_import_lowering_function(
            world_builder,
            module_builder,
            wasm_import_path,
            signature,
            path,
            sig,
        )?,
        ModuleArgument::Table => {
            todo!("implement the table import module arguments")
        }
    })
}
