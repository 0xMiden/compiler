use midenc_hir_macros::operation;

use crate::{dialects::hir::HirDialect, traits::*, *};

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryWrite)
)]
pub struct Store {
    #[operand]
    addr: AnyPointer,
    #[operand]
    value: AnyType,
}

// TODO(pauls): StoreLocal

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryRead),
    implements(InferTypeOpInterface)
)]
pub struct Load {
    #[operand]
    addr: AnyPointer,
    #[result]
    result: AnyType,
}

impl InferTypeOpInterface for Load {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        let span = self.span();
        let pointee = {
            let addr = self.addr();
            let addr_value = addr.value();
            addr_value.ty().pointee().cloned()
        };
        match pointee {
            Some(pointee) => {
                self.result_mut().set_type(pointee);
                Ok(())
            }
            None => {
                let addr = self.addr();
                let addr_value = addr.value();
                let addr_ty = addr_value.ty();
                Err(context
                    .session
                    .diagnostics
                    .diagnostic(miden_assembly::diagnostics::Severity::Error)
                    .with_message("invalid operand for 'load'")
                    .with_primary_label(
                        span,
                        format!("invalid 'addr' operand, expected pointer, got '{addr_ty}'"),
                    )
                    .into_report())
            }
        }
    }
}

// TODO(pauls): LoadLocal
