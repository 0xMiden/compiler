use midenc_hir_macros::operation;

use crate::{dialects::hir::HirDialect, traits::*, *};

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryRead, MemoryWrite, SameOperandsAndResultType)
)]
pub struct MemGrow {
    #[operand]
    pages: UInt32,
    #[result]
    result: UInt32,
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryRead)
)]
pub struct MemSize {
    #[result]
    result: UInt32,
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryWrite)
)]
pub struct MemSet {
    #[operand]
    addr: AnyPointer,
    #[operand]
    count: UInt32,
    #[operand]
    value: AnyType,
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryRead, MemoryWrite)
)]
pub struct MemCpy {
    #[operand]
    source: AnyPointer,
    #[operand]
    destination: AnyPointer,
    #[operand]
    count: UInt32,
}
