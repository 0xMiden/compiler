pub mod felt;
pub mod mem;

use std::{collections::HashSet, sync::OnceLock};

use midenc_hir::{FunctionIdent, FunctionType, SourceSpan, Symbol, Value};

use crate::module::function_builder_ext::FunctionBuilderExt;

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
pub fn convert_intrinsics_call(
    func_id: FunctionIdent,
    args: &[Value],
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
) -> Vec<Value> {
    match func_id.module.as_symbol().as_str() {
        mem::MODULE_ID => mem::convert_mem_intrinsics(func_id, args, builder, span),
        felt::MODULE_ID => felt::convert_felt_intrinsics(func_id, args, builder, span),
        _ => panic!("No intrinsics found for {}", func_id),
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

pub fn intrinsics_conversion_result(func_id: &FunctionIdent) -> IntrinsicsConversionResult {
    match func_id.module.as_symbol().as_str() {
        mem::MODULE_ID => {
            IntrinsicsConversionResult::FunctionType(intrinsic_function_type(func_id))
        }
        felt::MODULE_ID => IntrinsicsConversionResult::MidenVmOp,
        _ => panic!("No intrinsics conversion result found for {}", func_id),
    }
}
