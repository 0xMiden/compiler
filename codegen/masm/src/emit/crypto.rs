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

    /// Perform one FRI ext2 layer fold by a factor of four.
    pub fn fri_ext2fold4(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::FriExt2Fold4, 17, 16, span);
    }

    /// Perform eight Horner evaluation steps over base-field coefficients.
    pub fn horner_base(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::HornerBase, 16, 16, span);
    }

    /// Perform four Horner evaluation steps over extension-field coefficients.
    pub fn horner_ext(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::HornerExt, 16, 16, span);
    }

    /// Evaluate a memory-encoded arithmetic circuit and assert it evaluates to zero.
    pub fn eval_circuit(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::EvalCircuit, 3, 3, span);
    }

    /// Log a precompile event into the VM precompile transcript.
    pub fn log_precompile(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::LogPrecompile, 12, 12, span);
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

    #[test]
    fn proof_primitives_emit_vm_instructions_and_update_stack_shapes() {
        fn assert_transform(
            instruction: masm::Instruction,
            inputs: usize,
            outputs: usize,
            emit: impl FnOnce(&mut OpEmitter<'_>, SourceSpan),
        ) {
            let mut block = Vec::default();
            let context = Rc::new(Context::default());
            let mut stack = OperandStack::new(context);
            let mut invoked = BTreeSet::default();
            let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
            for _ in 0..inputs {
                emitter.push(Type::Felt);
            }

            let span = SourceSpan::default();
            emit(&mut emitter, span);

            assert_eq!(emitter.stack_len(), outputs);
            assert!(emitter.stack().iter().all(|ty| *ty == Type::Felt));
            assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, instruction)));
        }

        assert_transform(masm::Instruction::FriExt2Fold4, 17, 16, |emitter, span| {
            emitter.fri_ext2fold4(span);
        });
        assert_transform(masm::Instruction::HornerBase, 16, 16, |emitter, span| {
            emitter.horner_base(span);
        });
        assert_transform(masm::Instruction::HornerExt, 16, 16, |emitter, span| {
            emitter.horner_ext(span);
        });
        assert_transform(masm::Instruction::EvalCircuit, 3, 3, |emitter, span| {
            emitter.eval_circuit(span);
        });
        assert_transform(masm::Instruction::LogPrecompile, 12, 12, |emitter, span| {
            emitter.log_precompile(span);
        });
    }
}
