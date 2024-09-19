use crate::{dialects::hir::HirDialect, traits::*, *};

derive! {
    pub struct MemGrow : Op implements HasSideEffects, MemoryRead, MemoryWrite {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        pages: OpOperand,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct MemSize : Op implements HasSideEffects, MemoryRead {
        #[dialect]
        dialect: HirDialect,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct MemSet : Op implements HasSideEffects, MemoryWrite {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        addr: OpOperand,
        #[operand]
        count: OpOperand,
        #[operand]
        value: OpOperand,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct MemCpy : Op implements HasSideEffects, MemoryRead, MemoryWrite {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        source: OpOperand,
        #[operand]
        destination: OpOperand,
        #[operand]
        count: OpOperand,
        #[result]
        result: OpResult,
    }
}
