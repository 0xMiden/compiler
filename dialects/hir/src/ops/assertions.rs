use midenc_hir::{
    derive::operation, dialects::builtin::attributes::U32Attr, effects::*, traits::*, *,
};

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface)
)]
pub struct Assert {
    #[operand]
    value: Bool,
    #[attr]
    #[default]
    code: U32Attr,
}

impl EffectOpInterface<MemoryEffect> for Assert {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}

#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface)
)]
pub struct Assertz {
    #[operand]
    value: Bool,
    #[attr]
    #[default]
    code: U32Attr,
}

impl EffectOpInterface<MemoryEffect> for Assertz {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}

#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(MemoryEffectOpInterface)
)]
pub struct AssertEq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
}

impl EffectOpInterface<MemoryEffect> for AssertEq {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}
