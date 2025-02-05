use midenc_hir2::{derive::operation, traits::*, *};

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryRead, MemoryWrite, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct MemGrow {
    #[operand]
    pages: UInt32,
    #[result]
    result: UInt32,
}

impl InferTypeOpInterface for MemGrow {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryRead),
    implements(InferTypeOpInterface)
)]
pub struct MemSize {
    #[result]
    result: UInt32,
}

impl InferTypeOpInterface for MemSize {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
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
