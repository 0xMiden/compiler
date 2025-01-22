use midenc_hir::{
    diagnostics::{DiagnosticsHandler, Severity},
    AbiParam, CallConv, DataFlowGraph, FunctionIdent, FxHashMap, Ident, Linkage, Signature,
};

use super::{instance::ModuleArgument, ir_func_type, EntityIndex, FuncIndex, Module, ModuleTypes};
use crate::{
    error::WasmResult,
    intrinsics::is_miden_intrinsics_module,
    miden_abi::{is_miden_abi_module, miden_abi_function_type, recover_imported_masm_function_id},
    translation_utils::sig_from_func_type,
};

pub struct ModuleTranslationState {
    /// Imported and local functions
    /// Stores both the function reference and its signature
    functions: FxHashMap<FuncIndex, (FunctionIdent, Signature)>,
    /// Number of imported or aliased functions in the module.
    pub num_imported_funcs: usize,
    // stable_imported_miden_abi_functions: FxHashMap<FunctionIdent, String>,
}

impl ModuleTranslationState {
    pub fn new(
        module: &Module,
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
            let sig = sig_from_func_type(&ir_func_type, CallConv::SystemV, Linkage::External);
            if let Some(subst) = function_import_subst.get(&index) {
                functions.insert(index, (*subst, sig));
            } else if module.is_imported_function(index) {
                assert!((index.as_u32() as usize) < module.num_imported_funcs);
                let import = &module.imports[index.as_u32() as usize];
                let func_id =
                    recover_imported_masm_function_id(import.module.as_str(), &import.field);
                functions.insert(index, (func_id, sig));
            } else {
                let func_name = module.func_name(index);
                let func_id = FunctionIdent {
                    module: module.name(),
                    function: Ident::from(func_name.as_str()),
                };
                functions.insert(index, (func_id, sig));
            };
        }
        Self {
            functions,
            num_imported_funcs: module.num_imported_funcs,
        }
    }

    /// Returns an IR function signature converted from Wasm function signature
    /// for the given function index.
    pub fn signature(&self, index: FuncIndex) -> &Signature {
        &self.functions[&index].1
    }

    /// Get the `FunctionIdent` that should be used to make a direct call to function
    /// `index`.
    ///
    /// Import the callee into `func`'s DFG if it is not already present.
    pub(crate) fn get_direct_func(
        &mut self,
        dfg: &mut DataFlowGraph,
        index: FuncIndex,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<FunctionIdent> {
        let (func_id, wasm_sig) = self.functions[&index].clone();
        let (func_id, sig) = if is_miden_abi_module(func_id.module.as_symbol()) {
            let func_id = FunctionIdent {
                module: func_id.module,
                function: Ident::from(func_id.function.as_str().replace("-", "_").as_str()),
            };
            let ft =
                miden_abi_function_type(func_id.module.as_symbol(), func_id.function.as_symbol());
            (
                func_id,
                Signature::new(
                    ft.params.into_iter().map(AbiParam::new),
                    ft.results.into_iter().map(AbiParam::new),
                ),
            )
        } else {
            (func_id, wasm_sig.clone())
        };

        if is_miden_intrinsics_module(func_id.module.as_symbol()) {
            // Exit and do not import intrinsics functions into the DFG
            return Ok(func_id);
        }

        if dfg.get_import(&func_id).is_none() {
            dfg.import_function(func_id.module, func_id.function, sig.clone())
                .map_err(|_e| {
                    let message = format!(
                        "Function with name {} in module {} with signature {sig:?} is already \
                         imported (function call) with a different signature",
                        func_id.function, func_id.module
                    );
                    diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
                })?;
        }
        Ok(func_id)
    }
}
