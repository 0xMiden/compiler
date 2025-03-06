pub mod felt;
pub mod mem;

use std::{collections::HashSet, sync::OnceLock};

use midenc_hir::{
    dialects::builtin::{ModuleBuilder, WorldBuilder},
    interner::Symbol,
    FunctionIdent, FunctionType, Signature, SourceSpan, ValueRef,
};

use crate::{
    error::WasmResult,
    module::{
        function_builder_ext::FunctionBuilderExt, module_translation_state::CallableFunction,
    },
};

/// Check if the given module is a Miden module that contains intrinsics
pub fn is_miden_intrinsics_module(module_id: Symbol) -> bool {
    modules().contains(module_id.as_str())
}

fn modules() -> &'static HashSet<&'static str> {
    static MODULES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    MODULES.get_or_init(|| {
        let mut s = HashSet::default();
        s.insert(mem::MODULE_ID);
        s.insert(felt::MODULE_ID);
        s
    })
}

/// Convert a call to a Miden intrinsic function into instruction(s)
pub fn convert_intrinsics_call<B: ?Sized + Builder>(
    def_func: &CallableFunction,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    match def_func.wasm_id.module.as_symbol().as_str() {
        mem::MODULE_ID => mem::convert_mem_intrinsics(def_func, args, builder, span),
        felt::MODULE_ID => felt::convert_felt_intrinsics(def_func.wasm_id, args, builder, span),
        _ => panic!("No intrinsics found for {}", def_func.wasm_id),
    }
}

fn intrinsic_function_type(func_id: &FunctionIdent) -> FunctionType {
    match func_id.module.as_symbol().as_str() {
        mem::MODULE_ID => mem::function_type(func_id),
        _ => panic!("No intrinsics FunctionType found for {}", func_id),
    }
}

pub enum IntrinsicsConversionResult {
    FunctionType(FunctionType),
    MidenVmOp,
}

impl IntrinsicsConversionResult {
    pub fn is_function(&self) -> bool {
        matches!(self, IntrinsicsConversionResult::FunctionType(_))
    }

    pub fn is_operation(&self) -> bool {
        matches!(self, IntrinsicsConversionResult::MidenVmOp)
    }
}

pub fn intrinsics_conversion_result(func_id: &FunctionIdent) -> IntrinsicsConversionResult {
    match func_id.module.as_symbol().as_str() {
        mem::MODULE_ID => {
            IntrinsicsConversionResult::FunctionType(intrinsic_function_type(func_id))
        }
        felt::MODULE_ID => IntrinsicsConversionResult::MidenVmOp,
        _ => panic!("No intrinsics conversion result found for {}", func_id),
    }
}

/// Returns [`CallableFunction`] for a given intrinsics in core Wasm module imports
pub fn process_intrinsics_import(
    world_builder: &mut WorldBuilder,
    import_func_id: FunctionIdent,
    sig: Signature,
) -> CallableFunction {
    if intrinsics_conversion_result(&import_func_id).is_operation() {
        CallableFunction {
            wasm_id: import_func_id,
            // Call to this intrinsic functon will be translated as an IR op
            function_ref: None,
            signature: sig.clone(),
        }
    } else {
        // This intrinsic function will be defined further down the pipeline.
        // We are declaring it, creating the module if needed.
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
            .define_function(import_func_id.function, sig.clone())
            .expect("failed to create an import function");
        CallableFunction {
            wasm_id: import_func_id,
            function_ref: Some(import_func_ref),
            signature: sig,
        }
    }
}
