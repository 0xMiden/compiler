use alloc::format;

use midenc_hir2::{derive::operation, traits::*, *};

use crate::HirDialect;

/// Store `value` on the heap at `addr`
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

/// Load `result` from the heap at `addr`
///
/// The type of load is determined by the pointer operand type - cast the pointer to the type you
/// wish to load, so long as such a load is safe according to the semantics of your high-level
/// language.
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
                    .diagnostic(midenc_session::diagnostics::Severity::Error)
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
