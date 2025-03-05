use midenc_hir2::{derive::operation, effects::*, traits::*, *};
use smallvec::smallvec;

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    traits(SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct MemGrow {
    #[operand]
    pages: UInt32,
    #[result]
    result: UInt32,
}

impl EffectOpInterface<MemoryEffect> for MemGrow {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![
            EffectInstance::new(MemoryEffect::Read),
            EffectInstance::new(MemoryEffect::Write),
        ])
    }
}

impl InferTypeOpInterface for MemGrow {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct MemSize {
    #[result]
    result: UInt32,
}

impl EffectOpInterface<MemoryEffect> for MemSize {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Read),])
    }
}

impl InferTypeOpInterface for MemSize {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface)
)]
pub struct MemSet {
    #[operand]
    addr: AnyPointer,
    #[operand]
    count: UInt32,
    #[operand]
    value: AnyType,
}

impl EffectOpInterface<MemoryEffect> for MemSet {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new_for_value(
            MemoryEffect::Write,
            self.addr().as_value_ref()
        ),])
    }
}

#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface)
)]
pub struct MemCpy {
    #[operand]
    source: AnyPointer,
    #[operand]
    destination: AnyPointer,
    #[operand]
    count: UInt32,
}

impl EffectOpInterface<MemoryEffect> for MemCpy {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![
            EffectInstance::new_for_value(MemoryEffect::Read, self.source().as_value_ref()),
            EffectInstance::new_for_value(MemoryEffect::Write, self.destination().as_value_ref()),
        ])
    }
}
