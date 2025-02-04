use midenc_hir::diagnostics::{DiagnosticsHandler, Severity};
use midenc_hir2::{
    dialects::builtin::{Function, FunctionRef, ModuleBuilder},
    CallConv, FunctionIdent, FxHashMap, Ident, Signature, Symbol, SymbolName, SymbolRef,
    SymbolTable, UnsafeIntrusiveEntityRef, Visibility,
};

use super::{instance::ModuleArgument, ir_func_type, EntityIndex, FuncIndex, Module, ModuleTypes};
use crate::{
    error::WasmResult,
    intrinsics::is_miden_intrinsics_module,
    miden_abi::{is_miden_abi_module, miden_abi_function_type, recover_imported_masm_function_id},
    translation_utils::sig_from_func_type,
};

pub struct ModuleTranslationState<'a> {
    /// Imported and local functions
    /// Stores both the function reference and its signature
    functions: FxHashMap<FuncIndex, (FunctionIdent, Signature)>,
    pub module_builder: &'a mut ModuleBuilder,
}

impl<'a> ModuleTranslationState<'a> {
    pub fn new(
        module: &Module,
        module_builder: &'a mut ModuleBuilder,
        mod_types: &ModuleTypes,
        module_args: Vec<ModuleArgument>,
        diagnostics: &DiagnosticsHandler,
    ) -> Self {
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
            let sig = sig_from_func_type(&ir_func_type, CallConv::SystemV, Visibility::Public);
            if let Some(subst) = function_import_subst.get(&index) {
                functions.insert(index, (*subst, sig));
                todo!("define the import in some symbol table");
            } else if module.is_imported_function(index) {
                todo!("define the import in some symbol table");
                todo!("below");
                // assert!((index.as_u32() as usize) < module.num_imported_funcs);
                // let import = &module.imports[index.as_u32() as usize];
                // let func_id =
                //     recover_imported_masm_function_id(import.module.as_str(), &import.field);
                // functions.insert(index, (func_id, sig));
            } else {
                let func_name = module.func_name(index);
                let func_id = FunctionIdent {
                    module: Ident::from(module.name().as_str()),
                    function: Ident::from(func_name.as_str()),
                };
                functions.insert(index, (func_id, sig.clone()));
                module_builder
                    .define_function(func_id.function, sig)
                    .expect("adding new function failed");
            };
        }
        Self {
            functions,
            module_builder,
        }
    }

    /// Get the `FunctionIdent` that should be used to make a direct call to function
    /// `index`.
    ///
    /// Import the callee into `func`'s DFG if it is not already present.
    pub(crate) fn get_direct_func(
        &mut self,
        index: FuncIndex,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<FunctionRef> {
        // TODO: add error handling
        let (func_id, wasm_sig) = self.functions[&index].clone();
        assert_eq!(self.module_builder.module.borrow().name(), &func_id.module);
        let symbol_ref = self
            .module_builder
            .module
            .borrow()
            .get(func_id.function.as_symbol())
            .unwrap_or_else(|| {
                panic!(
                    "Function with name {} in module {} is not found",
                    func_id.function, func_id.module
                );
            });

        let op = symbol_ref.borrow();
        let func = op.as_symbol_operation().downcast_ref::<Function>().unwrap();
        let func_ref = func.as_function_ref();
        Ok(func_ref)

        // let function = symbol_ref.borrow().downcast_ref::<Function>().unwrap().as_symbol_ref();
        // let (func_id, wasm_sig) = self.functions[&index].clone();
        // let (func_id, sig) = if is_miden_abi_module(func_id.module.as_symbol()) {
        //     let func_id = FunctionIdent {
        //         module: func_id.module,
        //         function: Ident::from(func_id.function.as_str().replace("-", "_").as_str()),
        //     };
        //     let ft =
        //         miden_abi_function_type(func_id.module.as_symbol(), func_id.function.as_symbol());
        //     (
        //         func_id,
        //         Signature::new(
        //             ft.params.into_iter().map(AbiParam::new),
        //             ft.results.into_iter().map(AbiParam::new),
        //         ),
        //     )
        // } else {
        //     (func_id, wasm_sig.clone())
        // };
        //
        // if is_miden_intrinsics_module(func_id.module.as_symbol()) {
        //     // Exit and do not import intrinsics functions into the DFG
        //     return Ok(func_id);
        // }
        //
        // if dfg.get_import(&func_id).is_none() {
        //     dfg.import_function(func_id.module, func_id.function, sig.clone())
        //         .map_err(|_e| {
        //             let message = format!(
        //                 "Function with name {} in module {} with signature {sig:?} is already \
        //                  imported (function call) with a different signature",
        //                 func_id.function, func_id.module
        //             );
        //             diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
        //         })?;
        // }
        // Ok(func_id)
    }
}
