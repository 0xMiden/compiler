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
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        use midenc_session::diagnostics::Severity;
        let span = self.span();
        let lhs = self.first().ty().clone();
        {
            let rhs = self.second();
            if lhs != rhs.ty() {
                return Err(context
                    .session
                    .diagnostics
                    .diagnostic(Severity::Error)
                    .with_message("invalid operand types")
                    .with_primary_label(span, "operands of this operation are not compatible")
                    .with_secondary_label(
                        rhs.span(),
                        format!("expected this value to have type '{lhs}', but got '{}'", rhs.ty()),
                    )
                    .into_report());
            }
        }
        self.result_mut().set_type(lhs);
        Ok(())
    }
}
