use midenc_hir::{SourceSpan, Type};

use super::{OpEmitter, masm};

impl OpEmitter<'_> {
    /// Compute the Poseidon2 hash of one word.
    pub fn hash(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::Hash, 4, 4, span);
    }

    /// Compute the Poseidon2 merge hash of two words.
    pub fn hmerge(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::HMerge, 8, 4, span);
    }

    /// Apply the Poseidon2 permutation to the top three words.
    pub fn hperm(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::HPerm, 12, 12, span);
    }

    /// Encrypt two words from memory using the Poseidon2 sponge stream state.
    pub fn crypto_stream(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::CryptoStream, 14, 14, span);
    }

    fn felt_stack_transform(
        &mut self,
        instruction: masm::Instruction,
        inputs: usize,
        outputs: usize,
        span: SourceSpan,
    ) {
        for _ in 0..inputs {
            let operand = self.pop().expect("operand stack is empty");
            assert_eq!(operand.ty(), Type::Felt, "expected crypto operand to be felt");
        }
        self.emit(instruction, span);
        for _ in 0..outputs {
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
    fn hash_emits_vm_instruction_and_preserves_word_shape() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..4 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.hash(span);

        assert_eq!(emitter.stack_len(), 4);
        assert!(emitter.stack().iter().all(|ty| *ty == Type::Felt));
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::Hash)));
    }

    #[test]
    fn hmerge_emits_vm_instruction_and_returns_one_word() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..8 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.hmerge(span);

        assert_eq!(emitter.stack_len(), 4);
        assert!(emitter.stack().iter().all(|ty| *ty == Type::Felt));
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::HMerge)));
    }

    #[test]
    fn hperm_emits_vm_instruction_and_preserves_state_shape() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..12 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.hperm(span);

        assert_eq!(emitter.stack_len(), 12);
        assert!(emitter.stack().iter().all(|ty| *ty == Type::Felt));
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::HPerm)));
    }

    #[test]
    fn crypto_stream_emits_vm_instruction_and_preserves_window_shape() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..14 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.crypto_stream(span);

        assert_eq!(emitter.stack_len(), 14);
        assert!(emitter.stack().iter().all(|ty| *ty == Type::Felt));
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::CryptoStream)));
    }
}
