use midenc_hir::{
    derive::{EffectOpInterface, OpPrinter, operation},
    effects::*,
    traits::*,
    *,
};

use crate::*;

/// This operation represents a value produced by undefined behavior, i.e. control reaching a
/// program point that is not supposed to be reachable.
///
/// Any operation performed on a poison value, itself produces poison, and can be folded as such.
#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = UndefinedBehaviorDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct Poison {
    #[attr(hidden)]
    value: PoisonAttr,
    #[result]
    result: AnyType,
}

impl Foldable for Poison {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.value));
        FoldResult::Ok(())
    }

    fn fold_with(
        &self,
        _operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.value));
        FoldResult::Ok(())
    }
}

impl InferTypeOpInterface for Poison {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let poison_ty = self.get_value().clone();
        self.result_mut().set_type(poison_ty);
        Ok(())
    }
}

/// This operation represents an assertion that a specific program point should never be dynamically
/// reachable.
///
/// The specific way this gets lowered is up to the codegen backend and optimization choices.
#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = UndefinedBehaviorDialect,
    traits(Terminator),
    implements(MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct Unreachable {}
