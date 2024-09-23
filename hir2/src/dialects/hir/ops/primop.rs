use crate::{dialects::hir::HirDialect, traits::*, *};

derive! {
    pub struct MemGrow : Op {
        #[dialect]
        dialect: HirDialect,
        #[operand]
        pages: OpOperand,
        #[result]
        result: OpResult,
    }

    derives HasSideEffects, MemoryRead, MemoryWrite;
}

derive! {
    pub struct MemSize : Op {
        #[dialect]
        dialect: HirDialect,
        #[result]
        result: OpResult,
    }

    derives HasSideEffects, MemoryRead;
}

derive! {
    pub struct MemSet : Op {
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

    derives HasSideEffects, MemoryWrite;
}

derive! {
    pub struct MemCpy : Op {
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

    derives HasSideEffects, MemoryRead, MemoryWrite;
}
