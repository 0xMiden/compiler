use alloc::{string::String, sync::Arc};

use midenc_hir::{SourceSpan, Type};

use super::{OpEmitter, masm};

impl OpEmitter<'_> {
    /// Read a Merkle node and verify it against the provided root.
    pub fn mtree_get(&mut self, span: SourceSpan) {
        self.merkle_stack_transform(masm::Instruction::MTreeGet, 6, 8, span);
    }

    /// Update a Merkle node and return the old value plus new root.
    pub fn mtree_set(&mut self, span: SourceSpan) {
        self.merkle_stack_transform(masm::Instruction::MTreeSet, 10, 8, span);
    }

    /// Merge two Merkle roots and return the merged root.
    pub fn mtree_merge(&mut self, span: SourceSpan) {
        self.merkle_stack_transform(masm::Instruction::MTreeMerge, 8, 4, span);
    }

    /// Verify a Merkle path without changing the operand stack.
    pub fn mtree_verify(&mut self, message: Option<&str>, span: SourceSpan) {
        for index in 0..10 {
            let operand = self.stack.get(index).expect("operand stack is empty");
            assert_eq!(operand.ty(), Type::Felt, "expected Merkle operand to be felt");
        }
        let instruction = match message.filter(|message| !message.is_empty()) {
            Some(message) => Self::mtree_verify_with_message_inst(message.to_owned(), span),
            None => masm::Instruction::MTreeVerify,
        };
        self.emit(instruction, span);
    }

    fn merkle_stack_transform(
        &mut self,
        instruction: masm::Instruction,
        inputs: usize,
        outputs: usize,
        span: SourceSpan,
    ) {
        for _ in 0..inputs {
            let operand = self.pop().expect("operand stack is empty");
            assert_eq!(operand.ty(), Type::Felt, "expected Merkle operand to be felt");
        }
        self.emit(instruction, span);
        for _ in 0..outputs {
            self.push(Type::Felt);
        }
    }

    fn mtree_verify_with_message_inst(message: String, span: SourceSpan) -> masm::Instruction {
        masm::Instruction::MTreeVerifyWithError(masm::Immediate::Value(masm::Span::new(
            span,
            Arc::<str>::from(message),
        )))
    }
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeSet, rc::Rc};

    use midenc_hir::Context;

    use super::*;
    use crate::{OperandStack, masm::Op};

    fn emitter_with_felts(count: usize) -> (Vec<masm::Op>, OperandStack, BTreeSet<masm::Invoke>) {
        let block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        for _ in 0..count {
            stack.push(Type::Felt);
        }
        (block, stack, BTreeSet::default())
    }

    #[test]
    fn merkle_stack_transforms_emit_vm_instructions() {
        let span = SourceSpan::default();
        for (inputs, outputs, instruction) in [
            (6, 8, masm::Instruction::MTreeGet),
            (10, 8, masm::Instruction::MTreeSet),
            (8, 4, masm::Instruction::MTreeMerge),
        ] {
            let (mut block, mut stack, mut invoked) = emitter_with_felts(inputs);
            let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

            match instruction {
                masm::Instruction::MTreeGet => emitter.mtree_get(span),
                masm::Instruction::MTreeSet => emitter.mtree_set(span),
                masm::Instruction::MTreeMerge => emitter.mtree_merge(span),
                _ => unreachable!(),
            }

            assert_eq!(emitter.stack_len(), outputs);
            assert!(emitter.stack().iter().all(|ty| *ty == Type::Felt));
            assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, instruction)));
        }
    }

    #[test]
    fn mtree_verify_emits_vm_instruction_without_changing_stack() {
        let (mut block, mut stack, mut invoked) = emitter_with_felts(10);
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        let span = SourceSpan::default();

        emitter.mtree_verify(None, span);

        assert_eq!(emitter.stack_len(), 10);
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::MTreeVerify)));
    }

    #[test]
    fn mtree_verify_with_message_preserves_diagnostic() {
        let (mut block, mut stack, mut invoked) = emitter_with_felts(10);
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        let span = SourceSpan::default();

        emitter.mtree_verify(Some("bad path"), span);

        let Op::Inst(inst) = &block[0] else {
            panic!("expected instruction")
        };
        let masm::Instruction::MTreeVerifyWithError(masm::Immediate::Value(message)) = inst.inner()
        else {
            panic!("expected mtree_verify.err instruction")
        };
        assert_eq!(message.inner().as_ref(), "bad path");
    }
}
