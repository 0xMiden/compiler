use midenc_hir_macros::operation;

use crate::{dialects::hir::HirDialect, traits::*, *};

/// Choose a value based on a boolean condition
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface)
)]
pub struct Select {
    #[operand]
    cond: Bool,
    #[operand]
    first: AnyInteger,
    #[operand]
    second: AnyInteger,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for Select {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.first().ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}
