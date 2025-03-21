use midenc_hir::{
    dialects::builtin::{FunctionRef, ModuleBuilder, WorldBuilder},
    CallConv, FunctionIdent, FxHashMap, Ident, Signature, Visibility,
};
use midenc_session::diagnostics::DiagnosticsHandler;

use super::{instance::ModuleArgument, ir_func_type, types::ModuleTypesBuilder, FuncIndex, Module};
use crate::{
    component::lower_imports::generate_import_lowering_function,
    error::WasmResult,
    intrinsics::{is_miden_intrinsics_module, process_intrinsics_import},
    miden_abi::{
        define_func_for_miden_abi_transformation, is_miden_abi_module,
        recover_imported_masm_function_id,
    },
    translation_utils::sig_from_func_type,
};

/// Local core Wasm module functon or processed module import to be used for the translation of the
/// Wasm `call` op.
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
        module_args: FxHashMap<FunctionIdent, ModuleArgument>,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<Self> {
        let mut functions = FxHashMap::default();
        for (index, func_type) in &module.functions {
            let wasm_func_type = mod_types[func_type.signature].clone();
            let ir_func_type = ir_func_type(&wasm_func_type, diagnostics)?;
            let func_name = module.func_name(index);
            let func_id = FunctionIdent {
                module: Ident::from(module.name().as_str()),
                function: Ident::from(func_name.as_str()),
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
                    func_id,
                    sig,
                    import,
                    diagnostics,
                )?;
                functions.insert(index, func);
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
    pub(crate) fn get_direct_func(&mut self, index: FuncIndex) -> WasmResult<CallableFunction> {
        let defined_func = self.functions[&index].clone();
        Ok(defined_func)
    }
}

/// Returns [`CallableFunction`] translated from the core Wasm module import
fn process_import(
    module_builder: &mut ModuleBuilder,
    world_builder: &mut WorldBuilder,
    module_args: &hashbrown::HashMap<FunctionIdent, ModuleArgument, midenc_hir::FxBuildHasher>,
    core_func_id: FunctionIdent,
    core_func_sig: Signature,
    import: &super::ModuleImport,
    diagnostics: &DiagnosticsHandler,
) -> Result<CallableFunction, midenc_hir::Report> {
    let wasm_import_func_id = FunctionIdent {
        module: Ident::from(import.module.as_str()),
        function: Ident::from(import.field.as_str()),
    };
    let import_func_id = recover_imported_masm_function_id(import.module.as_str(), &import.field);
    let callable_function = if is_miden_intrinsics_module(import_func_id.module.as_symbol()) {
        process_intrinsics_import(world_builder, import_func_id, core_func_sig)
    } else if is_miden_abi_module(import_func_id.module.as_symbol()) {
        define_func_for_miden_abi_transformation(
            world_builder,
            module_builder,
            core_func_id,
            core_func_sig,
            import_func_id,
        )
    } else if let Some(module_arg) = module_args.get(&wasm_import_func_id) {
        process_module_arg(
            module_builder,
            world_builder,
            diagnostics,
            core_func_id,
            core_func_sig,
            wasm_import_func_id,
            module_arg,
        )?
    } else {
        panic!("unexpected import {import:?}");
    };
    Ok(callable_function)
}

fn process_module_arg(
    module_builder: &mut ModuleBuilder,
    world_builder: &mut WorldBuilder,
    diagnostics: &DiagnosticsHandler,
    func_id: FunctionIdent,
    sig: Signature,
    wasm_import_func_id: FunctionIdent,
    module_arg: &ModuleArgument,
) -> Result<CallableFunction, midenc_hir::Report> {
    Ok(match module_arg {
        ModuleArgument::Function(_function_ident) => {
            todo!("core Wasm function import is not implemented yet");
            //generate the internal function and call the import argument  function"
        }
        ModuleArgument::ComponentImport(signature) => generate_import_lowering_function(
            world_builder,
            module_builder,
            wasm_import_func_id,
            signature,
            func_id,
            sig,
            diagnostics,
        )?,
        ModuleArgument::Table => {
            todo!("implement the table import module arguments")
        }
    })
}
