use miden_assembly_syntax::parser::WordValue;
use midenc_dialect_hir::assertions;
use midenc_hir::{
    ArrayType, Felt, Immediate, SourceSpan, Type,
    dialects::builtin::attributes::{ArgumentExtension, Signature},
};

use super::{OpEmitter, int64, masm};
use crate::Event;

/// The message of the assertion guarding a `dyncall` against an unset stored-procedure slot.
///
/// An all-zero root word is what an account storage slot reads as before it is populated with a
/// sibling component's procedure root, so the guard reports it as such instead of leaving the VM
/// to fail later with "procedure not found". A transaction surfaces an account-code assertion only
/// as the error code derived from its message, so tests matching on that code need this exact
/// text; it is public for them.
pub const UNSET_STORED_PROCEDURE_SLOT_MESSAGE: &str =
    "stored procedure slot is unset: no procedure root to dyncall";

impl OpEmitter<'_> {
    /// Push the caller procedure hash as a word.
    pub fn caller(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::Caller, span);
        self.push(Type::from(ArrayType::new(Type::Felt, 4)));
    }

    /// Push the current VM clock cycle.
    pub fn clk(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::Clk, span);
        self.push(Type::Felt);
    }

    /// Format a diagnostic message for a HIR assertion code when one is available.
    fn assertion_message(
        code: Option<u32>,
        message: Option<&str>,
        default: impl Into<String>,
    ) -> String {
        if let Some(message) = message.filter(|message| !message.is_empty()) {
            return message.to_owned();
        }

        let default = default.into();
        match code.filter(|code| *code != 0) {
            Some(assertions::ASSERT_FAILED_ALIGNMENT) => {
                "pointer address does not meet minimum alignment for the type".into()
            }
            Some(code) => format!("{default} (assertion code 0x{code:08x})"),
            None => default,
        }
    }

    /// Assert that an integer value on the stack has the value 1
    ///
    /// This operation consumes the input value.
    pub fn assert(&mut self, code: Option<u32>, message: Option<&str>, span: SourceSpan) {
        let arg = self.stack.pop().expect("operand stack is empty");
        let ty = arg.ty().clone();
        let message =
            Self::assertion_message(code, message, format!("expected {ty} value to equal 1"));
        match ty {
            Type::Felt
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::U8
            | Type::I8
            | Type::I1 => {
                self.emit(Self::assert_with_message_inst(message, span), span);
            }
            Type::I128 | Type::U128 => {
                self.emit_all(
                    [
                        masm::Instruction::Push(masm::Immediate::Value(masm::Span::new(
                            span,
                            WordValue([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]).into(),
                        ))),
                        Self::assert_eqw_with_message_inst(message, span),
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => {
                self.emit_all(
                    [
                        Self::assertz_with_message_inst(message.clone(), span),
                        Self::assert_with_message_inst(message, span),
                    ],
                    span,
                );
            }
            ty if !ty.is_integer() => {
                panic!("invalid argument to assert: expected integer, got {ty}")
            }
            ty => unimplemented!("support for assert on {ty} is not implemented"),
        }
    }

    /// Assert that an integer value on the stack has the value 0
    ///
    /// This operation consumes the input value.
    pub fn assertz(&mut self, code: Option<u32>, message: Option<&str>, span: SourceSpan) {
        let arg = self.stack.pop().expect("operand stack is empty");
        let ty = arg.ty().clone();
        let message =
            Self::assertion_message(code, message, format!("expected {ty} value to equal 0"));
        match ty {
            Type::Felt
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::U8
            | Type::I8
            | Type::I1 => {
                self.emit(Self::assertz_with_message_inst(message, span), span);
            }
            Type::U64 | Type::I64 => {
                self.emit_all(
                    [
                        Self::assertz_with_message_inst(message.clone(), span),
                        Self::assertz_with_message_inst(message, span),
                    ],
                    span,
                );
            }
            Type::U128 | Type::I128 => {
                self.emit_all(
                    [
                        masm::Instruction::Push(masm::Immediate::Value(masm::Span::new(
                            span,
                            WordValue([Felt::ZERO; 4]).into(),
                        ))),
                        Self::assert_eqw_with_message_inst(message, span),
                    ],
                    span,
                );
            }
            ty if !ty.is_integer() => {
                panic!("invalid argument to assertz: expected integer, got {ty}")
            }
            ty => unimplemented!("support for assertz on {ty} is not implemented"),
        }
    }

    /// Assert that the top two integer values on the stack have the same value
    ///
    /// This operation consumes the input values.
    pub fn assert_eq(&mut self, code: Option<u32>, message: Option<&str>, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty().clone();
        assert_eq!(ty, rhs.ty(), "expected assert_eq operands to have the same type");
        let message =
            Self::assertion_message(code, message, format!("expected {ty} values to be equal"));
        match ty {
            Type::Felt
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::U8
            | Type::I8
            | Type::I1 => {
                self.emit(Self::assert_eq_with_message_inst(message, span), span);
            }
            Type::U128 | Type::I128 => {
                self.emit(Self::assert_eqw_with_message_inst(message, span), span)
            }
            Type::U64 | Type::I64 => {
                self.emit_all(
                    [
                        // compare the hi bits
                        masm::Instruction::MovUp2,
                        Self::assert_eq_with_message_inst(message.clone(), span),
                        // compare the low bits
                        Self::assert_eq_with_message_inst(message, span),
                    ],
                    span,
                );
            }
            ty if !ty.is_integer() => {
                panic!("invalid argument to assert_eq: expected integer, got {ty}")
            }
            ty => unimplemented!("support for assert_eq on {ty} is not implemented"),
        }
    }

    /// Emit code to assert that an integer value on the stack has the same value
    /// as the provided immediate.
    ///
    /// This operation consumes the input value.
    #[allow(unused)]
    pub fn assert_eq_imm(&mut self, imm: Immediate, span: SourceSpan) {
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty().clone();
        let message = format!("expected {ty} value to equal {imm}");
        assert_eq!(ty, imm.ty(), "expected assert_eq_imm operands to have the same type");
        match ty {
            Type::Felt
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::U8
            | Type::I8
            | Type::I1 => {
                self.emit_all(
                    [
                        masm::Instruction::EqImm(imm.as_felt().unwrap().into()),
                        Self::assert_with_message_inst(message, span),
                    ],
                    span,
                );
            }
            Type::I128 | Type::U128 => {
                self.push_immediate(imm, span);
                self.emit(Self::assert_eqw_with_message_inst(message, span), span)
            }
            Type::I64 | Type::U64 => {
                let imm = match imm {
                    Immediate::I64(i) => i as u64,
                    Immediate::U64(i) => i,
                    _ => unreachable!(),
                };
                let (hi, lo) = int64::to_raw_parts(imm);
                self.emit_all(
                    [
                        masm::Instruction::EqImm(Felt::new_unchecked(hi as u64).into()),
                        Self::assert_with_message_inst(message.clone(), span),
                        masm::Instruction::EqImm(Felt::new_unchecked(lo as u64).into()),
                        Self::assert_with_message_inst(message, span),
                    ],
                    span,
                )
            }
            ty if !ty.is_integer() => {
                panic!("invalid argument to assert_eq: expected integer, got {ty}")
            }
            ty => unimplemented!("support for assert_eq on {ty} is not implemented"),
        }
    }

    /// Emit code to select between two values of the same type, based on a boolean condition.
    ///
    /// The semantics of this instruction are basically the same as Miden's `cdrop` instruction,
    /// but with support for selecting between any of the representable integer/pointer types as
    /// values. Given three values on the operand stack (in order of appearance), `c`, `b`, and
    /// `a`:
    ///
    /// * Pop `c` from the stack. This value must be an i1/boolean, or execution will trap.
    /// * Pop `b` and `a` from the stack, and push back `b` if `c` is true, or `a` if `c` is false.
    ///
    /// This operation will assert that the selected value is a valid value for the given type.
    pub fn select(&mut self, span: SourceSpan) {
        let c = self.stack.pop().expect("operand stack is empty");
        let b = self.stack.pop().expect("operand stack is empty");
        let a = self.stack.pop().expect("operand stack is empty");
        assert_eq!(c.ty(), Type::I1, "expected selector operand to be an i1");
        let ty = a.ty();
        assert_eq!(ty, b.ty(), "expected selections to be of the same type");
        match &ty {
            Type::Felt
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::U8
            | Type::I8
            | Type::I1 => self.emit(masm::Instruction::CDrop, span),
            Type::I128 | Type::U128 => self.emit(masm::Instruction::CDropW, span),
            Type::I64 | Type::U64 => {
                // Perform two conditional drops, one for each 32-bit limb
                // corresponding to the value which is being selected
                self.emit_all(
                    [
                        // stack starts as [c, b_hi, b_lo, a_hi, a_lo]
                        masm::Instruction::Dup0, // [c, c, b_hi, b_lo, a_hi, a_lo]
                        masm::Instruction::MovDn5, // [c, b_hi, b_lo, a_hi, a_lo, c]
                        masm::Instruction::MovUp3, // [a_hi, c, b_hi, b_lo, a_lo, c]
                        masm::Instruction::MovUp2, // [b_hi, a_hi, c, b_lo, a_lo, c]
                        masm::Instruction::MovUp5, // [c, b_hi, a_hi, c, b_lo, a_lo]
                        masm::Instruction::CDrop, // [d_hi, c, b_lo, a_lo]
                        masm::Instruction::MovDn3, // [c, b_lo, a_lo, d_hi]
                        masm::Instruction::CDrop, // [d_lo, d_hi]
                        masm::Instruction::Swap1, // [d_hi, d_lo]
                    ],
                    span,
                );
            }
            ty if !ty.is_integer() => {
                panic!("invalid argument to assert_eq: expected integer, got {ty}")
            }
            ty => unimplemented!("support for assert_eq on {ty} is not implemented"),
        }
        self.push(ty);
    }

    /// Emit a `println` trace that can be handled by the debug executor.
    pub fn println(&mut self, span: SourceSpan) {
        // Don't `pop` operands as the debug executor reads them from the stack to handle printing.
        let ptr = &self.stack[0];
        let len = &self.stack[1];

        assert_eq!(
            ptr.ty(),
            Type::from(midenc_hir::PointerType::new(Type::U8)),
            "expected println pointer operand to be a ptr<u8>"
        );
        assert_eq!(len.ty(), Type::U32, "expected println length operand to be a u32");

        self.emit(masm::Instruction::EmitImm(Event::PrintLn.as_event_id().as_felt().into()), span);

        // Clean up the stack after the debug executor handled printing.
        self.dropn(2, span);
    }

    /// Execute the given procedure.
    ///
    /// A function called using this operation is invoked in the same memory context as the caller.
    pub fn exec(
        &mut self,
        callee: masm::InvocationTarget,
        signature: &Signature,
        span: SourceSpan,
    ) {
        self.process_call_signature(&callee, signature, span);

        self.emit(
            masm::Instruction::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
            span,
        );
        self.emit(masm::Instruction::Exec(callee), span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()), span);
    }

    /// Execute the procedure whose MAST root is stored in slot `index` (stack top) of a function
    /// table with `num_slots` slots based at `base_elem_addr` (a word-aligned element address).
    ///
    /// Traps with an assertion failure if `index >= num_slots`, or if the slot's signature tag
    /// differs from `type_tag` — which also covers null slots, whose tag is the reserved 0. The
    /// callee is invoked in the same memory context as the caller (`dynexec`).
    ///
    /// Expects `[index, args...]` on the operand stack, with the index on top. The index is
    /// rewritten in place to the slot's element address, which `dynexec` pops before
    /// transferring control, so the callee observes `[args...]` in normal argument order.
    pub fn exec_indirect(
        &mut self,
        num_slots: u32,
        base_elem_addr: u32,
        type_tag: u32,
        signature: &Signature,
        span: SourceSpan,
    ) {
        // Consume the index operand; all further effects on it are transient
        let index = self.stack.pop().expect("operand stack is empty");
        assert_eq!(index.ty(), Type::U32, "expected u32 table index for exec_indirect");

        // Bounds check: [index, ..] -> [index < num_slots, index, ..] -> [index, ..]
        self.emit(masm::Instruction::Dup0, span);
        self.emit_push(num_slots, span);
        self.emit(masm::Instruction::U32Lt, span);
        self.emit(
            Self::assert_with_message_inst(
                "indirect call: function table index out of bounds",
                span,
            ),
            span,
        );

        // Rewrite the index to the slot's element address: base_elem_addr + index * slot size.
        // The felt arithmetic cannot overflow: index < num_slots, and the linker guarantees
        // that the whole table fits in the 32-bit address space.
        self.emit(
            masm::Instruction::MulImm(
                Felt::new_unchecked(crate::linker::FunctionTableLayout::SLOT_SIZE_ELEMENTS as u64)
                    .into(),
            ),
            span,
        );
        self.emit(
            masm::Instruction::AddImm(Felt::new_unchecked(base_elem_addr as u64).into()),
            span,
        );

        // Signature check: the tag stored next to the slot's digest must equal the tag the call
        // site expects. A null slot keeps the zero tag that memory is initialized with, so it
        // can never match and traps here too.
        // [slot_addr, ..] -> [tag_addr, slot_addr, ..] -> [tag, slot_addr, ..] -> [slot_addr, ..]
        self.emit(masm::Instruction::Dup0, span);
        self.emit(
            masm::Instruction::AddImm(
                Felt::new_unchecked(
                    crate::linker::FunctionTableLayout::TYPE_TAG_OFFSET_ELEMENTS as u64,
                )
                .into(),
            ),
            span,
        );
        self.emit(masm::Instruction::MemLoad, span);
        self.emit_push(type_tag, span);
        self.emit(
            Self::assert_eq_with_message_inst(
                "indirect call: callee signature mismatch or null function reference",
                span,
            ),
            span,
        );

        self.consume_exact_call_signature(signature, "exec_indirect");

        // `dynexec` pops the element address and reads the callee MAST root word at it
        self.emit(
            masm::Instruction::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
            span,
        );
        self.emit(masm::Instruction::DynExec, span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()), span);
    }

    /// Check and spill the callee root of a `dyncall`, the first half of its lowering.
    ///
    /// Expects the root word on top of the operand stack, element 0 on top. The root is first
    /// checked to be non-zero — an all-zero root is an unset storage slot, reported with a
    /// descriptive assertion instead of the VM's late "procedure not found" failure — then
    /// spilled to the reserved scratch word at `root_scratch_addr` (see
    /// [crate::linker::ReservedCell::DyncallRoot]), as the VM reads a `dyncall` target from
    /// memory, and dropped. The arguments are scheduled afterwards, so the root and the
    /// arguments never have to share the addressable operand stack window.
    ///
    /// Reusing one fixed scratch word is safe from both sides of the call. From the callee's:
    /// the VM reads the root before the context switch, and the callee then runs in a fresh
    /// context where the caller's memory is invisible. From the caller's: nothing may write
    /// memory between the spill and the `dyncall` that reads it, and nothing does — the only
    /// code emitted in between is the operand scheduler's, placing the arguments, and its output
    /// is exclusively operand-stack manipulation (`Copy`/`Swap`/`MoveUp`/`MoveDown`; no memory
    /// instruction is emitted anywhere under `crate::opt::operands`). So no second `dyncall`
    /// lowering, and no other user of the cell, can interleave a write.
    pub fn dyncall_spill_root(&mut self, root_scratch_addr: u32, span: SourceSpan) {
        // Consume the root word; all further effects on it are transient
        for i in 0..midenc_dialect_hir::Dyncall::ROOT_FELTS {
            let root_felt = self.stack.pop().expect("operand stack is empty");
            assert_eq!(
                root_felt.ty(),
                Type::Felt,
                "expected felt root element {i} for dyncall, got {}",
                root_felt.ty()
            );
        }

        // Unset-slot guard: [r0, r1, r2, r3, ..] -> [all_zero, r0, r1, r2, r3, ..] -> [r0, ..]
        self.emit(masm::Instruction::Dup0, span);
        self.emit(masm::Instruction::EqImm(Felt::ZERO.into()), span);
        for dup in [masm::Instruction::Dup2, masm::Instruction::Dup3, masm::Instruction::Dup4] {
            self.emit(dup, span);
            self.emit(masm::Instruction::EqImm(Felt::ZERO.into()), span);
            self.emit(masm::Instruction::And, span);
        }
        self.emit(Self::assertz_with_message_inst(UNSET_STORED_PROCEDURE_SLOT_MESSAGE, span), span);

        // Spill the root to the scratch word: [r0, r1, r2, r3, ..] -> [..]
        //
        // The little-endian variant is the one that round-trips: it stores the stack top, `r0`,
        // at the lowest address, so the word reads back in element order — which is the order
        // `dyncall` expects of the digest at the address it pops.
        self.emit(masm::Instruction::MemStoreWLeImm(root_scratch_addr.into()), span);
        self.emit(masm::Instruction::DropW, span);
    }

    /// Dispatch a `dyncall` to the root spilled by [`Self::dyncall_spill_root`], the second half
    /// of its lowering.
    ///
    /// Expects `[args...]` on the operand stack in signature order. The scratch address is
    /// pushed on top and `dyncall` pops it before transferring control, so the callee observes
    /// `[args...]` in normal argument order and its results replace them.
    pub fn dyncall_dispatch(
        &mut self,
        root_scratch_addr: u32,
        signature: &Signature,
        span: SourceSpan,
    ) {
        // `dyncall` pops the element address and reads the callee MAST root word at it
        self.emit_push(root_scratch_addr, span);
        self.consume_exact_call_signature(signature, "dyncall");
        self.emit(
            masm::Instruction::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
            span,
        );
        self.emit(masm::Instruction::DynCall, span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()), span);
    }

    /// Push the MAST root digest of `callee` onto the operand stack as one word.
    ///
    /// This emits a `procref` instruction; the assembler computes the digest at assembly time
    /// and pushes it with `root[0]` on top.
    pub fn procedure_root(&mut self, callee: masm::InvocationTarget, span: SourceSpan) {
        for _ in 0..midenc_dialect_hir::ProcedureRoot::DIGEST_FELTS {
            self.push(Type::Felt);
        }
        self.emit(masm::Instruction::ProcRef(callee), span);
    }

    /// Execute the given procedure in a new context.
    ///
    /// A function called using this operation is invoked in a new memory context.
    pub fn call(
        &mut self,
        callee: masm::InvocationTarget,
        signature: &Signature,
        span: SourceSpan,
    ) {
        self.process_call_signature(&callee, signature, span);

        self.emit(
            masm::Instruction::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
            span,
        );
        self.emit(masm::Instruction::Call(callee), span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()), span);
    }

    /// Execute the given kernel procedure as a syscall.
    pub fn syscall(
        &mut self,
        callee: masm::InvocationTarget,
        signature: &Signature,
        span: SourceSpan,
    ) {
        self.process_call_signature(&callee, signature, span);

        self.emit(
            masm::Instruction::EmitImm(Event::FrameStart.as_event_id().as_felt().into()),
            span,
        );
        self.emit(masm::Instruction::SysCall(callee), span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.as_event_id().as_felt().into()), span);
    }

    /// Consumes one argument per parameter of `signature` from the emulated stack and produces
    /// its results, emitting no instructions.
    ///
    /// Used by the indirect-call lowerings (`exec_indirect`, `dyncall`), whose physical stack
    /// top holds the callee's memory address at this point: `process_call_signature`'s zext/sext
    /// paths emit instructions operating on that top, which is unavailable here. Requiring the
    /// argument types to match the parameter types exactly is what makes that irrelevant — a
    /// parameter's `zext`/`sext` is then a no-op, the same conclusion `process_call_signature`
    /// reaches in its `Zext | Sext => ()` arm — so no extension is inspected here. Legalization
    /// rejects the calls this cannot serve. `op_name` labels the assertion messages.
    fn consume_exact_call_signature(&mut self, signature: &Signature, op_name: &str) {
        for (i, param) in signature.params.iter().enumerate() {
            let arg = self.stack.pop().expect("operand stack is empty");
            assert_eq!(
                arg.ty(),
                param.ty,
                "invalid {op_name}: invalid argument type for parameter at index {i}"
            );
        }
        for result in signature.results.iter().rev() {
            self.push(result.ty.clone());
        }
    }

    fn process_call_signature(
        &mut self,
        callee: &masm::InvocationTarget,
        signature: &Signature,
        span: SourceSpan,
    ) {
        for i in 0..signature.arity() {
            let param = &signature.params[i];
            let arg = self.stack.pop().expect("operand stack is empty");
            let ty = arg.ty();
            // Validate the purpose matches
            if param.is_sret_param() {
                assert_eq!(
                    i, 0,
                    "invalid function signature: sret parameters must be the first parameter, and \
                     only one sret parameter is allowed"
                );
                assert_eq!(
                    signature.results.len(),
                    0,
                    "invalid function signature: a function with sret parameters cannot also have \
                     results"
                );
                assert!(
                    ty.is_pointer(),
                    "invalid exec to {callee}: invalid argument for sret parameter, expected {}, \
                     got {ty}",
                    param.ty
                );
            }
            // Validate that the argument type is valid for the parameter ABI
            match param.extension() {
                // Types must match exactly
                ArgumentExtension::None => {
                    assert_eq!(
                        ty, param.ty,
                        "invalid call to {callee}: invalid argument type for parameter at index \
                         {i}"
                    );
                }
                // Caller can provide a smaller type which will be zero-extended to the expected
                // type
                //
                // However, the argument must be an unsigned integer, and of smaller or equal size
                // in order for the types to differ
                ArgumentExtension::Zext if ty != param.ty => {
                    assert!(
                        param.ty.is_unsigned_integer(),
                        "invalid function signature: zero-extension is only valid for unsigned \
                         integer types"
                    );
                    assert!(
                        ty.is_unsigned_integer(),
                        "invalid call to {callee}: invalid argument type for parameter at index \
                         {i}, expected unsigned integer type, got {ty}"
                    );
                    let expected_size = param.ty.size_in_bits();
                    let provided_size = param.ty.size_in_bits();
                    assert!(
                        provided_size <= expected_size,
                        "invalid call to {callee}: invalid argument type for parameter at index \
                         {i}, expected integer width to be <= {expected_size} bits"
                    );
                    // Zero-extend this argument
                    self.stack.push(arg);
                    self.zext(&param.ty, span);
                    self.stack.drop();
                }
                // Caller can provide a smaller type which will be sign-extended to the expected
                // type
                //
                // However, the argument must be an integer which can fit in the range of the
                // expected type
                ArgumentExtension::Sext if ty != param.ty => {
                    assert!(
                        param.ty.is_signed_integer(),
                        "invalid function signature: sign-extension is only valid for signed \
                         integer types"
                    );
                    assert!(
                        ty.is_integer(),
                        "invalid call to {callee}: invalid argument type for parameter at index \
                         {i}, expected integer type, got {ty}"
                    );
                    let expected_size = param.ty.size_in_bits();
                    let provided_size = param.ty.size_in_bits();
                    if ty.is_unsigned_integer() {
                        assert!(
                            provided_size < expected_size,
                            "invalid call to {callee}: invalid argument type for parameter at \
                             index {i}, expected unsigned integer width to be < {expected_size} \
                             bits"
                        );
                    } else {
                        assert!(
                            provided_size <= expected_size,
                            "invalid call to {callee}: invalid argument type for parameter at \
                             index {i}, expected integer width to be <= {expected_size} bits"
                        );
                    }
                    // Push the operand back on the stack for `sext`
                    self.stack.push(arg);
                    self.sext(&param.ty, span);
                    self.stack.drop();
                }
                ArgumentExtension::Zext | ArgumentExtension::Sext => (),
            }
        }

        for result in signature.results.iter().rev() {
            self.push(result.ty.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeSet, rc::Rc};

    use midenc_hir::{ArrayType, Context};

    use super::*;
    use crate::{OperandStack, masm::Op};

    #[test]
    fn caller_emits_vm_instruction_and_pushes_word() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let span = SourceSpan::default();
        emitter.caller(span);

        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::from(ArrayType::new(Type::Felt, 4)));
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::Caller)));
    }

    /// Pin the exact instruction sequence and stack effect of an indirect call: the bounds
    /// check, the in-place index-to-address rewrite, the signature-tag check, and the
    /// frame-traced `dynexec`.
    #[test]
    fn exec_indirect_emits_bounds_check_tag_check_and_dynexec() {
        use midenc_hir::{CallConv, Felt};

        use crate::linker::FunctionTableLayout;

        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let signature =
            Signature::with_convention(&context, CallConv::C, [Type::I32, Type::I32], [Type::I32]);

        // The scheduled operand order is [index, args...], index on top
        emitter.push(Type::I32);
        emitter.push(Type::I32);
        emitter.push(Type::U32);

        let span = SourceSpan::default();
        let num_slots = 5u32;
        let base_elem_addr = 294912u32;
        let type_tag = 3u32;
        emitter.exec_indirect(num_slots, base_elem_addr, type_tag, &signature, span);

        // The emulated stack holds exactly the call result
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::I32);

        let insts = block
            .iter()
            .map(|op| match op {
                Op::Inst(inst) => inst.clone().into_inner(),
                op => panic!("unexpected non-instruction op: {op:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(insts.len(), 14);
        // Bounds check: duplicate the index and assert it is in bounds
        assert_eq!(insts[0], masm::Instruction::Dup0);
        assert!(
            matches!(&insts[1], masm::Instruction::Push(masm::Immediate::Value(value)) if *value.inner() == num_slots.into()),
            "expected push of the slot count, got {:?}",
            insts[1]
        );
        assert_eq!(insts[2], masm::Instruction::U32Lt);
        assert!(
            matches!(&insts[3], masm::Instruction::AssertWithError(masm::Immediate::Value(msg)) if msg.inner().contains("function table index out of bounds")),
            "expected bounds-check assertion, got {:?}",
            insts[3]
        );
        // Rewrite the index to the slot's element address
        assert!(
            matches!(&insts[4], masm::Instruction::MulImm(masm::Immediate::Value(value)) if *value.inner() == Felt::new_unchecked(FunctionTableLayout::SLOT_SIZE_ELEMENTS as u64)),
            "expected multiply by the slot size, got {:?}",
            insts[4]
        );
        assert!(
            matches!(&insts[5], masm::Instruction::AddImm(masm::Immediate::Value(value)) if *value.inner() == Felt::new_unchecked(base_elem_addr as u64)),
            "expected add of the table base address, got {:?}",
            insts[5]
        );
        // Signature check: load the slot's tag and assert it matches the expected tag
        assert_eq!(insts[6], masm::Instruction::Dup0);
        assert!(
            matches!(&insts[7], masm::Instruction::AddImm(masm::Immediate::Value(value)) if *value.inner() == Felt::new_unchecked(FunctionTableLayout::TYPE_TAG_OFFSET_ELEMENTS as u64)),
            "expected add of the tag offset, got {:?}",
            insts[7]
        );
        assert_eq!(insts[8], masm::Instruction::MemLoad);
        assert!(
            matches!(&insts[9], masm::Instruction::Push(masm::Immediate::Value(value)) if *value.inner() == type_tag.into()),
            "expected push of the expected signature tag, got {:?}",
            insts[9]
        );
        assert!(
            matches!(&insts[10], masm::Instruction::AssertEqWithError(masm::Immediate::Value(msg)) if msg.inner().contains("callee signature mismatch")),
            "expected signature-check assertion, got {:?}",
            insts[10]
        );
        // Frame-traced dynexec, which itself pops the slot address
        assert!(matches!(&insts[11], masm::Instruction::EmitImm(_)));
        assert_eq!(insts[12], masm::Instruction::DynExec);
        assert!(matches!(&insts[13], masm::Instruction::EmitImm(_)));
    }

    /// Pin the exact instruction sequence and stack effect of a dynamic cross-context call: the
    /// unset-slot guard, the root spill to the scratch word, the address push, and the
    /// frame-traced `dyncall`. The two halves are emitted back to back here; the lowering
    /// schedules the arguments in between.
    #[test]
    fn dyncall_emits_unset_guard_root_spill_and_dyncall() {
        use midenc_hir::{CallConv, Felt};

        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let signature = Signature::with_convention(
            &context,
            CallConv::ComponentModel,
            [Type::Felt, Type::U32],
            [Type::U32],
        );

        // The scheduled operand order is [root0..root3, args...], root element 0 on top
        emitter.push(Type::U32);
        emitter.push(Type::Felt);
        for _ in 0..midenc_dialect_hir::Dyncall::ROOT_FELTS {
            emitter.push(Type::Felt);
        }

        let span = SourceSpan::default();
        let scratch = crate::linker::ReservedCell::DyncallRoot.element_addr();
        emitter.dyncall_spill_root(scratch, span);
        emitter.dyncall_dispatch(scratch, &signature, span);

        // The emulated stack holds exactly the call result
        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::U32);
        // No static invocation target is recorded for the assembler call graph
        assert!(invoked.is_empty());

        let insts = block
            .iter()
            .map(|op| match op {
                Op::Inst(inst) => inst.clone().into_inner(),
                op => panic!("unexpected non-instruction op: {op:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(insts.len(), 18);
        // Unset-slot guard: fold `root[i] == 0` over the word, then assert the fold is false
        let is_eq_zero = |inst: &masm::Instruction| matches!(inst, masm::Instruction::EqImm(masm::Immediate::Value(value)) if *value.inner() == Felt::ZERO);
        assert_eq!(insts[0], masm::Instruction::Dup0);
        assert!(is_eq_zero(&insts[1]), "expected eq.0, got {:?}", insts[1]);
        for (i, dup) in [masm::Instruction::Dup2, masm::Instruction::Dup3, masm::Instruction::Dup4]
            .into_iter()
            .enumerate()
        {
            let base = 2 + i * 3;
            assert_eq!(insts[base], dup);
            assert!(is_eq_zero(&insts[base + 1]), "expected eq.0, got {:?}", insts[base + 1]);
            assert_eq!(insts[base + 2], masm::Instruction::And);
        }
        assert!(
            matches!(&insts[11], masm::Instruction::AssertzWithError(masm::Immediate::Value(msg)) if &**msg.inner() == UNSET_STORED_PROCEDURE_SLOT_MESSAGE),
            "expected unset-slot assertion, got {:?}",
            insts[11]
        );
        // Spill the root to the scratch word and drop it
        assert!(
            matches!(&insts[12], masm::Instruction::MemStoreWLeImm(masm::Immediate::Value(addr)) if *addr.inner() == scratch),
            "expected store of the root to the scratch word, got {:?}",
            insts[12]
        );
        assert_eq!(insts[13], masm::Instruction::DropW);
        // Push the scratch address, then the frame-traced dyncall pops it
        assert!(
            matches!(&insts[14], masm::Instruction::Push(masm::Immediate::Value(value)) if *value.inner() == scratch.into()),
            "expected push of the scratch address, got {:?}",
            insts[14]
        );
        assert!(matches!(&insts[15], masm::Instruction::EmitImm(_)));
        assert_eq!(insts[16], masm::Instruction::DynCall);
        assert!(matches!(&insts[17], masm::Instruction::EmitImm(_)));
    }

    #[test]
    fn clk_emits_vm_instruction_and_pushes_felt() {
        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        let span = SourceSpan::default();
        emitter.clk(span);

        assert_eq!(emitter.stack_len(), 1);
        assert_eq!(emitter.stack()[0], Type::Felt);
        assert_eq!(&block[0], &Op::Inst(masm::Span::new(span, masm::Instruction::Clk)));
    }
}
