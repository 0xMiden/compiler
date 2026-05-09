use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::ImmediateAttr,
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

/// Emit an event whose ID is already present on the operand stack.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct EmitEvent {
    #[operand]
    event_id: IntFelt,
    #[result]
    result: IntFelt,
}

impl InferTypeOpInterface for EmitEvent {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::Felt);
        Ok(())
    }
}

/// Emit an event identified by an immediate field element.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct EmitEventImm {
    #[attr]
    event_id: ImmediateAttr,
}
