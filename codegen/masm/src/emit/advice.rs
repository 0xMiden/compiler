use midenc_hir::{SourceSpan, Type};

use super::{OpEmitter, masm};

impl OpEmitter<'_> {
    /// Pop one felt from the advice stack to the operand stack.
    pub fn advice_pop(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::AdvPush(1u8.into()), span);
        self.push(Type::Felt);
    }

    /// Pop one word from the advice stack, overwriting the top four stack slots.
    pub fn advice_load_word(&mut self, span: SourceSpan) {
        for _ in 0..4 {
            self.pop().expect("operand stack is empty");
        }
        self.emit(masm::Instruction::AdvLoadW, span);
        for _ in 0..4 {
            self.push(Type::Felt);
        }
    }

    /// Pop two advice words, write them to memory, and update the top-13 stack window.
    pub fn advice_pipe(&mut self, span: SourceSpan) {
        for _ in 0..13 {
            let operand = self.pop().expect("operand stack is empty");
            assert_eq!(operand.ty(), Type::Felt, "expected advice_pipe operand to be felt");
        }
        self.emit(masm::Instruction::AdvPipe, span);
        for _ in 0..13 {
            self.push(Type::Felt);
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeSet, rc::Rc};

    use midenc_hir::Context;

    use super::*;
    use crate::{OperandStack, masm::Op};

    #[test]
    fn advice_pop_emits_adv_push_one_and_updates_stack() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let span = SourceSpan::default();
        emitter.advice_pop(span);

        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(
            &block[0],
            &Op::Inst(masm::Span::new(span, masm::Instruction::AdvPush(1u8.into())))
        );
    }

    #[test]
    fn advice_load_word_emits_adv_loadw_and_replaces_four_slots() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..4 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.advice_load_word(span);

        assert_eq!(emitter.stack_len(), 4);
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::AdvLoadW)));
    }

    #[test]
    fn advice_pipe_emits_vm_instruction_and_preserves_window_shape() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..13 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.advice_pipe(span);

        assert_eq!(emitter.stack_len(), 13);
        assert!(emitter.stack().iter().all(|ty| *ty == Type::Felt));
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::AdvPipe)));
    }
}
