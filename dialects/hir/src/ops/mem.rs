use midenc_hir2::{derive::operation, effects::*, traits::*, *};
use smallvec::smallvec;

use crate::HirDialect;

/// Store `value` on the heap at `addr`
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface)
)]
pub struct Store {
    #[operand]
    addr: AnyPointer,
    #[operand]
    value: AnyType,
}

impl EffectOpInterface<MemoryEffect> for Store {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new_for_value(
            MemoryEffect::Write,
            self.addr().as_value_ref()
        )])
    }
}

// TODO(pauls): StoreLocal

/// Load `result` from the heap at `addr`
///
/// The type of load is determined by the pointer operand type - cast the pointer to the type you
/// wish to load, so long as such a load is safe according to the semantics of your high-level
/// language.
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct Load {
    #[operand]
    addr: AnyPointer,
    #[result]
    result: AnyType,
}

impl EffectOpInterface<MemoryEffect> for Load {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new_for_value(
            MemoryEffect::Read,
            self.addr().as_value_ref()
        )])
    }
}

impl InferTypeOpInterface for Load {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let _span = self.span();
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
                // let addr = self.addr();
                // let addr_value = addr.value();
                // let addr_ty = addr_value.ty();
                // Err(context
                //     .session
                //     .diagnostics
                //     .diagnostic(midenc_session::diagnostics::Severity::Error)
                //     .with_message("invalid operand for 'load'")
                //     .with_primary_label(
                //         span,
                //         format!("invalid 'addr' operand, expected pointer, got '{addr_ty}'"),
                //     )
                //     .into_report())
                Ok(())
            }
        }
    }
}

// TODO(pauls): LoadLocal
