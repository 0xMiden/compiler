use midenc_hir::{
    derive::{EffectOpInterface, operation},
    effects::MemoryEffectOpInterface,
    traits::*,
    *,
};

use crate::*;

/// An operation for expressing constant immediate values.
///
/// This is used to materialize folded constants for the arithmetic dialect.
#[derive(EffectOpInterface)]
#[operation(
    dialect = ArithDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct Constant {
    #[attr(hidden)]
    value: ImmediateAttr,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for Constant {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.value().ty().clone();
        self.result_mut().set_type(ty);

        Ok(())
    }
}

impl Foldable for Constant {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        results.push(OpFoldResult::Attribute(self.value));
        FoldResult::Ok(())
    }

    #[inline(always)]
    fn fold_with(
        &self,
        _operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        self.fold(results)
    }
}

impl OpPrinter for Constant {
    fn print(&self, printer: &mut print::AsmPrinter<'_>) {
        printer.print_space();
        printer.print_decimal_integer(*self.get_value());
        printer.print_space();
        printer.print_colon_type(self.result().ty());
    }
}
