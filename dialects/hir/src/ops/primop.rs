use midenc_hir::{
    derive::{EffectOpInterface, OpPrinter, operation},
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
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

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read))]
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

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct MemSet {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    addr: AnyPointer,
    #[operand]
    count: UInt32,
    #[operand]
    value: AnyType,
}

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface)
)]
pub struct MemCpy {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read))]
    source: AnyPointer,
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    destination: AnyPointer,
    #[operand]
    count: UInt32,
}

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct Breakpoint {}
