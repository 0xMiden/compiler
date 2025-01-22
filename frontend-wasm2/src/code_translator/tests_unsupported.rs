use midenc_hir::{CallConv, Linkage, ModuleBuilder, Signature, SourceSpan};
use wasmparser::{MemArg, Operator, Operator::*};

use super::translate_operator;
use crate::{
    module::{
        func_translation_state::FuncTranslationState,
        function_builder_ext::{FunctionBuilderContext, FunctionBuilderExt},
        module_translation_state::ModuleTranslationState,
        Module,
    },
    test_utils::test_context,
};

fn check_unsupported(op: &Operator) {
    let context = test_context();
    let mod_name = "noname";
    let module_info = Module::default();
    let mut module_builder = ModuleBuilder::new(mod_name);
    let sig = Signature {
        params: vec![],
        results: vec![],
        cc: CallConv::SystemV,
        linkage: Linkage::External,
    };
    let mut module_func_builder = module_builder.function("func_name", sig.clone()).unwrap();
    let mut fb_ctx = FunctionBuilderContext::new();
    let mod_types = Default::default();
    let mut state = FuncTranslationState::new();
    let mut builder_ext = FunctionBuilderExt::new(&mut module_func_builder, &mut fb_ctx);
    let mut module_state =
        ModuleTranslationState::new(&module_info, &mod_types, vec![], &context.session.diagnostics);
    let result = translate_operator(
        op,
        &mut builder_ext,
        &mut state,
        &mut module_state,
        &module_info,
        &mod_types,
        &context.session.diagnostics,
        SourceSpan::default(),
    );
    assert!(result.is_err(), "Expected unsupported op error for {:?}", op);
    assert_eq!(result.unwrap_err().to_string(), format!("Wasm op {:?} is not supported", op));
}

// Wasm Spec v1.0
const UNSUPPORTED_WASM_V1_OPS: &[Operator] = &[
    /****************************** Memory Operators *********************************** */
    F64Load {
        memarg: MemArg {
            align: 0,
            max_align: 0,
            offset: 0,
            memory: 0,
        },
    },
    F64Store {
        memarg: MemArg {
            align: 0,
            max_align: 0,
            offset: 0,
            memory: 0,
        },
    },
    /****************************** Nullary Operators ************************************/

    // Cannot construct since Ieee32 fields are private
    // F32Const {
    //     value: Ieee32(0),
    // },
    // F64Const {
    //     value: Ieee32(0),
    // },

    /****************************** Unary Operators ************************************/
    F32Sqrt,
    F64Sqrt,
    F32Ceil,
    F64Ceil,
    F32Floor,
    F64Floor,
    F32Trunc,
    F64Trunc,
    F32Nearest,
    F64Nearest,
    F32Abs,
    F64Abs,
    F32Neg,
    F64Neg,
    F64ConvertI64U,
    F64ConvertI32U,
    F64ConvertI64S,
    F64ConvertI32S,
    F32ConvertI64S,
    F32ConvertI32S,
    F32ConvertI64U,
    F32ConvertI32U,
    F64PromoteF32,
    F32DemoteF64,
    I64TruncF64S,
    I64TruncF32S,
    I32TruncF64S,
    I32TruncF32S,
    I64TruncF64U,
    I64TruncF32U,
    I32TruncF64U,
    I32TruncF32U,
    I64TruncSatF64S,
    I64TruncSatF32S,
    I32TruncSatF64S,
    I32TruncSatF32S,
    I64TruncSatF64U,
    I64TruncSatF32U,
    I32TruncSatF64U,
    I32TruncSatF32U,
    F32ReinterpretI32,
    F64ReinterpretI64,
    I32ReinterpretF32,
    I64ReinterpretF64,
    /****************************** Binary Operators *********************************** */
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Min,
    F32Max,
    F32Copysign,
    F64Copysign,
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Min,
    F64Max,
    /**************************** Comparison Operators ********************************* */
    F32Eq,
    F32Ne,
    F32Gt,
    F32Ge,
    F32Le,
    F32Lt,
    F64Eq,
    F64Ne,
    F64Gt,
    F64Ge,
    F64Le,
    F64Lt,
];

#[test]
fn error_for_unsupported_wasm_v1_ops() {
    for op in UNSUPPORTED_WASM_V1_OPS.iter() {
        check_unsupported(op);
    }
}
