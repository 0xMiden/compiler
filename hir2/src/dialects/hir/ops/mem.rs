use crate::{dialects::hir::HirDialect, traits::*, *};

derive! {
    pub struct Store : Op implements HasSideEffects, MemoryWrite {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        addr: OpOperand,
        #[operand]
        value: OpOperand,
    }
}

// TODO(pauls): StoreLocal

derive! {
    pub struct Load : Op implements HasSideEffects, MemoryRead {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        addr: OpOperand,
    }
}

// TODO(pauls): LoadLocal
