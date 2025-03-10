use alloc::boxed::Box;

use midenc_hir2::{derive::operation, effects::MemoryEffectOpInterface, traits::*, *};

use crate::*;

/// An operation for expressing constant immediate values.
///
/// This is used to materialize folded constants for the arithmetic dialect.
#[operation(
    dialect = ArithDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct Constant {
    #[attr(hidden)]
    value: Immediate,
    #[result]
    result: AnyInteger,
}

has_no_effects!(Constant);

impl InferTypeOpInterface for Constant {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.value().ty();
        self.result_mut().set_type(ty);

        Ok(())
    }
}

impl Foldable for Constant {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.get_attribute("value").unwrap().clone_value()));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}
