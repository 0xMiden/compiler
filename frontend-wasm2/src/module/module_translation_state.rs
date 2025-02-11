use midenc_hir::diagnostics::{DiagnosticsHandler, Severity};
use midenc_hir2::{
    dialects::builtin::{ComponentBuilder, Function, FunctionRef, ModuleBuilder},
    CallConv, FunctionIdent, FxHashMap, Ident, Signature, Symbol, SymbolName, SymbolNameComponent,
    SymbolPath, SymbolRef, SymbolTable, UnsafeIntrusiveEntityRef, Visibility,
};

use super::{instance::ModuleArgument, ir_func_type, EntityIndex, FuncIndex, Module, ModuleTypes};
use crate::{
    error::WasmResult,
    intrinsics::is_miden_intrinsics_module,
    miden_abi::{is_miden_abi_module, miden_abi_function_type, recover_imported_masm_function_id},
    translation_utils::sig_from_func_type,
};

#[derive(Clone, Debug)]
struct DefinedFunction {
    wasm_id: FunctionIdent,
    symbol_path: SymbolPath,
    signature: Signature,
}

pub struct ModuleTranslationState<'a> {
    /// Imported and local functions
    /// Stores both the function reference and its signature
    functions: FxHashMap<FuncIndex, DefinedFunction>,
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
                // functions.insert(index, (*subst, sig));
                todo!("define the import in some symbol table");
            } else if module.is_imported_function(index) {
                assert!((index.as_u32() as usize) < module.num_imported_funcs);
                let import = &module.imports[index.as_u32() as usize];
                let func_id =
                    recover_imported_masm_function_id(import.module.as_str(), &import.field);
                component_builder.define_import(func_id, sig.clone());

                let root_component = component_builder.component.borrow().name().as_symbol();
                let path = function_ident_to_sympol_path(root_component, &func_id);
                let defined_function = DefinedFunction {
                    wasm_id: func_id,
                    symbol_path: path,
                    signature: sig,
                };
                functions.insert(index, defined_function);
            } else {
                let func_name = module.func_name(index);
                let func_id = FunctionIdent {
                    module: Ident::from(module.name().as_str()),
                    function: Ident::from(func_name.as_str()),
                };
                let root_component = component_builder.component.borrow().name().as_symbol();
                let path = function_ident_to_sympol_path(root_component, &func_id);
                let defined_function = DefinedFunction {
                    wasm_id: func_id,
                    symbol_path: path,
                    signature: sig.clone(),
                };
                functions.insert(index, defined_function);
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
        let defined_func = self.functions[&index].clone();
        let symbol_ref = self
            .module_builder
            .module
            .borrow()
            .resolve(&defined_func.symbol_path)
            .unwrap_or_else(|| {
                panic!("Failed to resolve function {:?}", defined_func);
            });

        let op = symbol_ref.borrow();
        let func = op
            .as_symbol_operation()
            .downcast_ref::<Function>()
            .expect("expected resolved symbol to be a Function");
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

fn function_ident_to_sympol_path(
    root_component: SymbolName,
    func_id: &FunctionIdent,
) -> SymbolPath {
    SymbolPath::new(vec![
        SymbolNameComponent::Root,
        SymbolNameComponent::Component(root_component),
        SymbolNameComponent::Component(func_id.module.as_symbol()),
        SymbolNameComponent::Leaf(func_id.function.as_symbol()),
    ])
    .unwrap()
}
