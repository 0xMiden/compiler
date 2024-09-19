use crate::{dialects::hir::HirDialect, traits::*, *};

// TODO(pauls): Implement support for:
//
// * Inferring op constraints from callee signature
derive! {
    pub struct Exec : Op implements CallInterface {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        callee: FunctionIdent,
    }
}

derive! {
    pub struct ExecIndirect : Op implements CallInterface {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        signature: Signature,
        #[operand]
        callee: OpOperand,
    }
}
