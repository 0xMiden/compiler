use alloc::format;

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

/// Emit a recognized VM system event.
///
/// MASM system events leave the operand stack unchanged, but they read a variant-specific window
/// from the top of the operand stack. The operands/results of this op represent that stack window
/// in top-to-bottom order so SSA captures the event's data dependencies.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct SystemEvent {
    #[operands]
    stack: IntFelt,
    #[attr]
    event_id: ImmediateAttr,
    #[results]
    outputs: IntFelt,
}

impl InferTypeOpInterface for SystemEvent {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        if !matches!(*self.get_event_id(), Immediate::Felt(_)) {
            return Err(Report::msg("hir.system_event event_id must be a felt immediate"));
        }

        let num_results = self.stack().len();
        if !self.op.results.is_empty() && self.op.results.len() != num_results {
            return Err(Report::msg(format!(
                "invalid hir.system_event: expected {num_results} result(s), but got {}",
                self.op.results.len()
            )));
        }

        if self.op.results.is_empty() {
            let span = self.span();
            let owner = self.as_operation_ref();
            for i in 0..num_results {
                let value = context.make_result(span, Type::Felt, owner, i as u8);
                self.op.results.push(value);
            }
        } else {
            for result in self.op.results.iter_mut() {
                result.borrow_mut().set_type(Type::Felt);
            }
        }

        Ok(())
    }
}
