use midenc_hir::{Report, SourceSpan, Type};

use super::{OpEmitter, masm};

impl OpEmitter<'_> {
    /// Pop one felt from the advice stack to the operand stack.
    pub fn advice_pop(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::AdvPush, span);
        self.push(Type::Felt);
    }

    /// Replace four one-lane operands with one word from the advice stack.
    ///
    /// Each input must have a nonempty representation occupying exactly one raw lane; pointer and
    /// packed types are permitted. Validate all four inputs before changing the emitted body or
    /// typed stack, then describe the four replacement lanes as Felts. The VM can still trap if the
    /// advice provider cannot supply the word.
    pub fn advice_load_word(&mut self, span: SourceSpan) -> Result<(), Report> {
        if self.stack().len() < 4 {
            return Err(Report::msg("advice_load_word requires four operands"));
        }
        for index in 0..4 {
            let operand = self.stack().get(index).unwrap();
            if operand.size() != 1 || operand.ty().size_in_bytes() == 0 {
                return Err(Report::msg(alloc::format!(
                    "advice_load_word operand {index} ({}): expected one nonempty raw slot",
                    operand.ty()
                )));
            }
        }
        for _ in 0..4 {
            self.pop().expect("operand stack is empty");
        }
        self.emit(masm::Instruction::AdvLoadW, span);
        for _ in 0..4 {
            self.push(Type::Felt);
        }
        Ok(())
    }

    /// Pop two advice words, write them to memory, and update the top-13 stack window.
    pub fn advice_pipe(&mut self, span: SourceSpan) {
        self.felt_stack_transform(masm::Instruction::AdvPipe, 13, 13, "advice_pipe", span);
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
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::AdvPush)));
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
        emitter.advice_load_word(span).unwrap();

        assert_eq!(emitter.stack_len(), 4);
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::AdvLoadW)));
    }

    #[test]
    fn advice_load_word_rejects_wide_before_mutation() {
        let mut cases = (0..4).map(|n| vec![Type::Felt; n]).collect::<Vec<_>>();
        for ty in [
            Type::U64,
            Type::I64,
            Type::U128,
            Type::from(midenc_hir::ArrayType::new(Type::Felt, 4)),
        ] {
            // Zero-sized types are rejected by ValueLayout before an Operand can be built.
            // Exercise emitter validation with constructible, nonempty multi-lane operands.
            for index in 0..4 {
                let mut types = vec![Type::Felt; 4];
                types[index] = ty.clone();
                cases.push(types);
            }
        }
        for types in cases {
            let mut block = Vec::default();
            let mut stack = OperandStack::new(Rc::new(Context::default()));
            let mut invoked = BTreeSet::default();
            for ty in types {
                stack.push(ty);
            }
            let before = stack.iter().map(|v| v.ty()).collect::<Vec<_>>();
            let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
            assert!(emitter.advice_load_word(SourceSpan::UNKNOWN).is_err());
            assert_eq!(emitter.stack().iter().map(|v| v.ty()).collect::<Vec<_>>(), before);
            assert!(block.is_empty());
            assert!(invoked.is_empty());
        }
    }

    #[test]
    fn advice_load_word_accepts_mixed_pointer_and_packed_inputs() {
        use midenc_hir::{AddressSpace, ArrayType, PointerType};
        let mut block = Vec::new();
        let mut stack = OperandStack::new(Rc::new(Context::default()));
        let mut invoked = BTreeSet::new();
        stack.push(Type::U64);
        for ty in [
            Type::U8,
            Type::from(PointerType::new(Type::U128)),
            Type::from(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element)),
            Type::from(ArrayType::new(Type::U8, 4)),
        ] {
            stack.push(ty);
        }
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);
        emitter.advice_load_word(SourceSpan::UNKNOWN).unwrap();
        assert_eq!(emitter.stack_len(), 5);
        assert!((0..4).all(|i| emitter.stack().get(i).unwrap().ty() == Type::Felt));
        assert_eq!(emitter.stack().get(4).unwrap().ty(), Type::U64);
        assert_eq!(
            block,
            [Op::Inst(masm::Span::new(SourceSpan::UNKNOWN, masm::Instruction::AdvLoadW))]
        );
        assert!(invoked.is_empty());
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
