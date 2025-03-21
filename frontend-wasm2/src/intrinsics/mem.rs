use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    AbiParam, Builder, FunctionIdent, FunctionType, Signature, SourceSpan, Type, ValueRef,
};

use crate::{
    error::WasmResult,
    module::{
        function_builder_ext::FunctionBuilderExt, module_translation_state::CallableFunction,
    },
};

pub const MODULE_ID: &str = "intrinsics::mem";

pub const HEAP_BASE: &str = "heap_base";

const HEAP_BASE_FUNC: ([Type; 0], [Type; 1]) = ([], [Type::U32]);

pub fn function_type(func_id: &FunctionIdent) -> FunctionType {
    match func_id.function.as_symbol().as_str() {
        HEAP_BASE => FunctionType::new(HEAP_BASE_FUNC.0, HEAP_BASE_FUNC.1),
        _ => panic!("No memory intrinsics FunctionType found for {}", func_id),
    }
}

fn signature(func_id: &FunctionIdent) -> Signature {
    match func_id.function.as_symbol().as_str() {
        HEAP_BASE => {
            Signature::new(HEAP_BASE_FUNC.0.map(AbiParam::new), HEAP_BASE_FUNC.1.map(AbiParam::new))
        }
        _ => panic!("No memory intrinsics Signature found for {}", func_id),
    }
}

/// Convert a call to a memory intrinsic function
pub(crate) fn convert_mem_intrinsics<B: ?Sized + Builder>(
    def_func: &CallableFunction,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    match def_func.wasm_id.function.as_symbol().as_str() {
        HEAP_BASE => {
            assert_eq!(args.len(), 0, "{} takes no arguments", def_func.wasm_id);

            let func_ref = def_func.function_ref.unwrap_or_else(|| {
                panic!("expected DefinedFunction::function_ref to be set for {}", def_func.wasm_id)
            });
            let exec = builder.exec(func_ref, def_func.signature.clone(), args.to_vec(), span)?;
            let borrow = exec.borrow();
            let results = borrow.as_ref().results();
            let result_vals: Vec<ValueRef> =
                results.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
            Ok(result_vals)
        }
        _ => panic!("No allowed memory intrinsics found for {}", def_func.wasm_id),
    }
}
