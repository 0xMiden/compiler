use midenc_hir::{Felt, SourceSpan, Type};

use super::{OpEmitter, masm};

impl OpEmitter<'_> {
    /// Emit an event whose felt ID is already on top of the operand stack.
    pub fn emit_event(&mut self, span: SourceSpan) {
        let event_id = self.stack.peek().expect("operand stack is empty");
        assert_eq!(event_id.ty(), Type::Felt, "expected event id to be felt");
        self.emit(masm::Instruction::Emit, span);
    }

    /// Emit an event identified by an immediate felt.
    pub fn emit_event_imm(&mut self, event_id: Felt, span: SourceSpan) {
        self.emit(
            masm::Instruction::EmitImm(masm::Immediate::Value(masm::Span::new(span, event_id))),
            span,
        );
    }

    /// Emit a recognized VM system event without changing the operand stack.
    pub fn system_event(&mut self, event_id: Felt, read_count: usize, span: SourceSpan) {
        for index in 0..read_count {
            let operand = self.stack.get(index).expect("operand stack is empty");
            assert_eq!(operand.ty(), Type::Felt, "expected system event operand to be felt");
        }
        self.emit_event_imm(event_id, span);
    }
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeSet, rc::Rc};

    use midenc_hir::Context;

    use super::*;
    use crate::{OperandStack, masm::Op};

    #[test]
    fn emit_event_emits_emit_without_changing_stack() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        emitter.push(Type::Felt);

        let span = SourceSpan::default();
        emitter.emit_event(span);

        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::Emit)));
    }

    #[test]
    fn emit_event_imm_emits_immediate_event_without_changing_stack() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let span = SourceSpan::default();
        emitter.emit_event_imm(Felt::new(42), span);

        assert_eq!(emitter.stack_len(), 0);
        assert_eq!(
            &block[0],
            &Op::Inst(masm::Span::new(
                span,
                masm::Instruction::EmitImm(masm::Immediate::Value(masm::Span::new(
                    span,
                    Felt::new(42),
                )))
            ))
        );
    }

    #[test]
    fn system_event_emits_immediate_event_without_changing_stack() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..4 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.system_event(Felt::new(7), 4, span);

        assert_eq!(emitter.stack_len(), 4);
        assert_eq!(
            &block[0],
            &Op::Inst(masm::Span::new(
                span,
                masm::Instruction::EmitImm(masm::Immediate::Value(masm::Span::new(
                    span,
                    Felt::new(7),
                )))
            ))
        );
    }
}
