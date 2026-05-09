use midenc_hir::{SourceSpan, Type};

use super::{OpEmitter, masm};

impl OpEmitter<'_> {
    pub fn ext2add(&mut self, span: SourceSpan) {
        self.ext2_binary(masm::Instruction::Ext2Add, span);
    }

    pub fn ext2sub(&mut self, span: SourceSpan) {
        self.ext2_binary(masm::Instruction::Ext2Sub, span);
    }

    pub fn ext2mul(&mut self, span: SourceSpan) {
        self.ext2_binary(masm::Instruction::Ext2Mul, span);
    }

    pub fn ext2div(&mut self, span: SourceSpan) {
        self.ext2_binary(masm::Instruction::Ext2Div, span);
    }

    pub fn ext2neg(&mut self, span: SourceSpan) {
        self.ext2_unary(masm::Instruction::Ext2Neg, span);
    }

    pub fn ext2inv(&mut self, span: SourceSpan) {
        self.ext2_unary(masm::Instruction::Ext2Inv, span);
    }

    fn ext2_binary(&mut self, instruction: masm::Instruction, span: SourceSpan) {
        let rhs0 = self.pop().expect("operand stack is empty");
        let rhs1 = self.pop().expect("operand stack is empty");
        let lhs0 = self.pop().expect("operand stack is empty");
        let lhs1 = self.pop().expect("operand stack is empty");
        assert_eq!(rhs0.ty(), Type::Felt, "expected ext2 operand limb to be felt");
        assert_eq!(rhs1.ty(), Type::Felt, "expected ext2 operand limb to be felt");
        assert_eq!(lhs0.ty(), Type::Felt, "expected ext2 operand limb to be felt");
        assert_eq!(lhs1.ty(), Type::Felt, "expected ext2 operand limb to be felt");
        self.emit(instruction, span);
        self.push(Type::Felt);
        self.push(Type::Felt);
    }

    fn ext2_unary(&mut self, instruction: masm::Instruction, span: SourceSpan) {
        let operand0 = self.pop().expect("operand stack is empty");
        let operand1 = self.pop().expect("operand stack is empty");
        assert_eq!(operand0.ty(), Type::Felt, "expected ext2 operand limb to be felt");
        assert_eq!(operand1.ty(), Type::Felt, "expected ext2 operand limb to be felt");
        self.emit(instruction, span);
        self.push(Type::Felt);
        self.push(Type::Felt);
    }
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeSet, rc::Rc};

    use midenc_hir::Context;

    use super::*;
    use crate::{OperandStack, masm::Op};

    #[test]
    fn ext2_binary_emits_vm_instruction_and_updates_stack() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..4 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.ext2add(span);

        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::Felt);
        assert_eq!(emitter.stack()[1], Type::Felt);
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::Ext2Add)));
    }

    #[test]
    fn ext2_unary_emits_vm_instruction_and_updates_stack() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        for _ in 0..2 {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        emitter.ext2inv(span);

        assert_eq!(emitter.stack_len(), 2);
        assert_eq!(emitter.stack()[0], Type::Felt);
        assert_eq!(emitter.stack()[1], Type::Felt);
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::Ext2Inv)));
    }
}
