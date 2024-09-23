use crate::{dialects::hir::HirDialect, traits::*, *};

derive! {
    pub struct Store : Op {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        addr: OpOperand,
        #[operand]
        value: OpOperand,
    }

    derives HasSideEffects, MemoryWrite;
}

// TODO(pauls): StoreLocal

derive! {
    pub struct Load : Op {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        addr: OpOperand,
    }

    derives HasSideEffects, MemoryRead;
}

// TODO(pauls): LoadLocal
