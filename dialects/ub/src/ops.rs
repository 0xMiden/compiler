use alloc::boxed::Box;

use midenc_hir2::{derive::operation, effects::*, traits::*, *};

use crate::*;

/// This operation represents a value produced by undefined behavior, i.e. control reaching a
/// program point that is not supposed to be reachable.
///
/// Any operation performed on a poison value, itself produces poison, and can be folded as such.
#[operation(
    dialect = UndefinedBehaviorDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct Poison {
    #[attr(hidden)]
    value: PoisonAttr,
    #[result]
    result: AnyType,
}

impl EffectOpInterface<MemoryEffect> for Poison {
    fn has_no_effect(&self) -> bool {
        true
    }

    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec::smallvec![])
    }
}

impl Foldable for Poison {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(Box::new(self.value().clone())));
        FoldResult::Ok(())
    }

    fn fold_with(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        results.push(OpFoldResult::Attribute(Box::new(self.value().clone())));
        FoldResult::Ok(())
    }
}

impl InferTypeOpInterface for Poison {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let poison_ty = self.value().ty().clone();
        self.result_mut().set_type(poison_ty);
        Ok(())
    }
}

/// This operation represents an assertion that a specific program point should never be dynamically
/// reachable.
///
/// The specific way this gets lowered is up to the codegen backend and optimization choices.
#[operation(
    dialect = UndefinedBehaviorDialect,
    traits(Terminator),
    implements(MemoryEffectOpInterface)
)]
pub struct Unreachable {}

impl EffectOpInterface<MemoryEffect> for Unreachable {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}
