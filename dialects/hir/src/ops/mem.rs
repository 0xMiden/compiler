use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::LocalVariableAttr,
    effects::*,
    traits::*,
    *,
};
use midenc_hir_transform::SpillLike;

use crate::HirDialect;

/// Store `value` on the heap at `addr`
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
pub struct Store {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    addr: AnyPointer,
    #[operand]
    value: AnyType,
}

/// Store `value` on in procedure local memory
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, SpillLike, OpPrinter)
)]
pub struct StoreLocal {
    #[attr]
    #[effects(MemoryEffect(MemoryEffect::Write))]
    local: LocalVariableAttr,
    #[operand]
    value: AnyType,
}

impl SpillLike for StoreLocal {
    fn spilled(&self) -> OpOperand {
        self.value().as_operand_ref()
    }

    fn spilled_value(&self) -> ValueRef {
        self.value().as_value_ref()
    }
}

/// Load `result` from the heap at `addr`
///
/// The type of load is determined by the pointer operand type - cast the pointer to the type you
/// wish to load, so long as such a load is safe according to the semantics of your high-level
/// language.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Load {
    #[operand]
    #[effects(MemoryEffect(MemoryEffect::Read))]
    addr: AnyPointer,
    #[result]
    result: AnyType,
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

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct LoadLocal {
    #[attr]
    #[effects(MemoryEffect(MemoryEffect::Read))]
    local: LocalVariableAttr,
    #[result]
    result: AnyType,
}

impl InferTypeOpInterface for LoadLocal {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.get_local().ty();
        self.result_mut().set_type(ty);

        Ok(())
    }
}
