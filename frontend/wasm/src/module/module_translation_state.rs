use midenc_hir::{
    dialects::builtin::{ModuleBuilder, WorldBuilder},
    interner::Symbol,
    smallvec, CallConv, FxHashMap, Signature, SymbolNameComponent, SymbolPath, Visibility,
};
use midenc_session::diagnostics::DiagnosticsHandler;

use super::{instance::ModuleArgument, ir_func_type, types::ModuleTypesBuilder, FuncIndex, Module};
use crate::{
    callable::CallableFunction,
    component::lower_imports::generate_import_lowering_function,
    error::WasmResult,
    intrinsics::{process_intrinsics_import, Intrinsic},
    miden_abi::{
        define_func_for_miden_abi_transformation, is_miden_abi_module,
        recover_imported_masm_function_id,
    },
    translation_utils::sig_from_func_type,
};

pub struct ModuleTranslationState<'a> {
    /// Imported and local functions
    functions: FxHashMap<FuncIndex, CallableFunction>,
    pub module_builder: &'a mut ModuleBuilder,
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
                let func =
                    process_import(module_builder, world_builder, &module_args, path, sig, import)?;
                functions.insert(index, func);
            } else {
                let function_ref = module_builder
                    .define_function(path.name().into(), sig.clone())
                    .expect("adding new function failed");
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
        })
    }

    /// Get the `CallableFunction` that should be used to make a direct call to function `index`.
    pub(crate) fn get_direct_func(&mut self, index: FuncIndex) -> WasmResult<CallableFunction> {
        let defined_func = self.functions[&index].clone();
        Ok(defined_func)
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
) -> Result<CallableFunction, midenc_hir::Report> {
    match recover_imported_masm_function_id(&import.module, &import.field) {
        Some(masm_function_path) => {
            if let Ok(intrinsic) = Intrinsic::try_from(&masm_function_path) {
                Ok(process_intrinsics_import(world_builder, intrinsic, core_func_sig))
            } else if is_miden_abi_module(&masm_function_path) {
                Ok(define_func_for_miden_abi_transformation(
                    world_builder,
                    module_builder,
                    core_func_id,
                    core_func_sig,
                    masm_function_path,
                ))
            } else {
                unimplemented!("unhandled masm primitive: '{masm_function_path}'")
            }
        }
        None => {
            let import_path = SymbolPath {
                path: smallvec![
                    SymbolNameComponent::Root,
                    SymbolNameComponent::Component(Symbol::intern(&import.module)),
                    SymbolNameComponent::Leaf(Symbol::intern(&import.field))
                ],
            };
            let module_arg = module_args
                .get(&import_path)
                .unwrap_or_else(|| panic!("unexpected import '{import_path:?}'"));
            process_module_arg(
                module_builder,
                world_builder,
                core_func_id,
                core_func_sig,
                import_path,
                module_arg,
            )
        }
    }
}

fn process_module_arg(
    module_builder: &mut ModuleBuilder,
    world_builder: &mut WorldBuilder,
    path: SymbolPath,
    sig: Signature,
    wasm_import_path: SymbolPath,
    module_arg: &ModuleArgument,
) -> Result<CallableFunction, midenc_hir::Report> {
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
