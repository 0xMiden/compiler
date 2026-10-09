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
/// as the error code derived from its message, which makes the exact text load-bearing; naming it
/// here spells it once for the emitter below and the unit test asserting what it emits.
pub(crate) const UNSET_STORED_PROCEDURE_SLOT_MESSAGE: &str =
    "stored procedure slot is unset: no procedure root to dyncall";

/// The number of operand stack elements a `call`, `syscall` or `dyncall` hands to the callee,
/// and that the callee hands back.
const CALL_WINDOW_FELTS: usize = miden_core::program::MIN_STACK_DEPTH;

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
                        // The first element of a pushed word ends on top, as the least significant
                        // limb does
                        masm::Instruction::Push(masm::Immediate::Value(masm::Span::new(
                            span,
                            WordValue([Felt::ONE, Felt::ZERO, Felt::ZERO, Felt::ZERO]).into(),
                        ))),
                        Self::assert_eqw_with_message_inst(message, span),
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => {
                self.emit_all(
                    [
                        // The low limb, on top, is 1, and the high limb is 0
                        Self::assert_with_message_inst(message.clone(), span),
                        Self::assertz_with_message_inst(message, span),
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
                        // compare the low limbs
                        masm::Instruction::MovUp2,
                        Self::assert_eq_with_message_inst(message.clone(), span),
                        // compare the high limbs
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
                        // The low limb is on top
                        masm::Instruction::EqImm(Felt::new_unchecked(lo as u64).into()),
                        Self::assert_with_message_inst(message.clone(), span),
                        masm::Instruction::EqImm(Felt::new_unchecked(hi as u64).into()),
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
                        // stack starts as [c, b_lo, b_hi, a_lo, a_hi]
                        masm::Instruction::Dup0, // [c, c, b_lo, b_hi, a_lo, a_hi]
                        masm::Instruction::MovDn5, // [c, b_lo, b_hi, a_lo, a_hi, c]
                        masm::Instruction::MovUp3, // [a_lo, c, b_lo, b_hi, a_hi, c]
                        masm::Instruction::MovUp2, // [b_lo, a_lo, c, b_hi, a_hi, c]
                        masm::Instruction::MovUp5, // [c, b_lo, a_lo, c, b_hi, a_hi]
                        masm::Instruction::CDrop, // [d_lo, c, b_hi, a_hi]
                        masm::Instruction::MovDn3, // [c, b_hi, a_hi, d_lo]
                        masm::Instruction::CDrop, // [d_hi, d_lo]
                        masm::Instruction::Swap1, // [d_lo, d_hi]
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

        self.emit(masm::Instruction::EmitImm(Event::PrintLn.into()), span);

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

        self.emit(masm::Instruction::EmitImm(Event::FrameStart.into()), span);
        self.emit(masm::Instruction::Exec(callee), span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.into()), span);
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
        self.emit(masm::Instruction::EmitImm(Event::FrameStart.into()), span);
        self.emit(masm::Instruction::DynExec, span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.into()), span);
    }

    /// Check and spill the callee root of a `dyncall`, the first half of its lowering.
    ///
    /// Expects the root word on top of the operand stack, element 0 on top. The root is first
    /// checked to be non-zero — an all-zero root is an unset storage slot, reported with a
    /// descriptive assertion instead of the VM's late "procedure not found" failure — then
    /// spilled to the reserved scratch word at `root_scratch_addr` (see
    /// [crate::linker::DYNCALL_ROOT_ADDR]), as the VM reads a `dyncall` target from
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

        // Unset-slot guard: [r0, r1, r2, r3, ..] -> [all_zero, r0, r1, r2, r3, ..]
        //                                        -> [r0, r1, r2, r3, ..]
        //
        // The accumulated flag rides the stack top throughout the fold, shifting every root
        // element down by one, so `r1`, `r2` and `r3` are reached at depth 2, 3 and 4 rather than
        // 1, 2 and 3. `assertz` then consumes the flag, leaving the whole root word untouched for
        // the spill below.
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
    ///
    /// Like [`Self::call`], the callee receives exactly `[args, zeros]` as its 16-element window,
    /// and the window's padding is discarded on return (see [`Self::pad_call_window`]).
    pub fn dyncall_dispatch(
        &mut self,
        root_scratch_addr: u32,
        signature: &Signature,
        span: SourceSpan,
    ) {
        let (num_arg_felts, num_result_felts) = Self::call_window_felts(signature);
        self.pad_call_window(num_arg_felts, span);
        // `dyncall` pops the element address and reads the callee MAST root word at it
        self.emit_push(root_scratch_addr, span);
        self.consume_exact_call_signature(signature, "dyncall");
        self.emit(masm::Instruction::EmitImm(Event::FrameStart.into()), span);
        self.emit(masm::Instruction::DynCall, span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.into()), span);
        self.discard_call_window_padding(num_result_felts, span);
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
    ///
    /// The VM hands the callee exactly the top 16 operand stack elements and replaces them with
    /// the 16 elements the callee returns. Callees follow the `[args, pad]` → `[results, pad]`
    /// convention and may clobber anything under their arguments, so the window is padded to
    /// `[args, zeros]` before the call, keeping every caller value below it, and the padding the
    /// callee leaves under its results is discarded afterwards (see [`Self::pad_call_window`]).
    pub fn call(
        &mut self,
        callee: masm::InvocationTarget,
        signature: &Signature,
        span: SourceSpan,
    ) {
        let (num_arg_felts, num_result_felts) = Self::call_window_felts(signature);
        self.process_call_signature(&callee, signature, span);
        self.pad_call_window(num_arg_felts, span);

        self.emit(masm::Instruction::EmitImm(Event::FrameStart.into()), span);
        self.emit(masm::Instruction::Call(callee), span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.into()), span);
        self.discard_call_window_padding(num_result_felts, span);
    }

    /// Execute the given kernel procedure as a syscall.
    ///
    /// A `syscall` has the same 16-element window semantics as a `call`, so the window is padded
    /// and its padding discarded exactly as described in [`Self::call`].
    pub fn syscall(
        &mut self,
        callee: masm::InvocationTarget,
        signature: &Signature,
        span: SourceSpan,
    ) {
        let (num_arg_felts, num_result_felts) = Self::call_window_felts(signature);
        self.process_call_signature(&callee, signature, span);
        self.pad_call_window(num_arg_felts, span);

        self.emit(masm::Instruction::EmitImm(Event::FrameStart.into()), span);
        self.emit(masm::Instruction::SysCall(callee), span);
        self.emit(masm::Instruction::EmitImm(Event::FrameEnd.into()), span);
        self.discard_call_window_padding(num_result_felts, span);
    }

    /// Returns the number of argument and result field elements a cross-context invocation of a
    /// procedure with `signature` passes through the 16-element call window.
    ///
    /// Panics if either does not fit in the window: the frontend passes wider argument or result
    /// lists indirectly, so a wider signature here is a compiler bug.
    fn call_window_felts(signature: &Signature) -> (usize, usize) {
        let num_arg_felts: usize = signature.params.iter().map(|p| p.ty.size_in_felts()).sum();
        let num_result_felts: usize = signature.results.iter().map(|r| r.ty.size_in_felts()).sum();
        assert!(
            num_arg_felts <= CALL_WINDOW_FELTS,
            "cross-context call arguments take {num_arg_felts} field elements, but the call \
             window holds at most {CALL_WINDOW_FELTS}"
        );
        assert!(
            num_result_felts <= CALL_WINDOW_FELTS,
            "cross-context call results take {num_result_felts} field elements, but the call \
             window holds at most {CALL_WINDOW_FELTS}"
        );
        (num_arg_felts, num_result_felts)
    }

    /// Pad the `num_arg_felts` argument elements on top of the operand stack with zeros, so that
    /// the 16-element window a `call`/`syscall`/`dyncall` hands to the callee is exactly
    /// `[args, zeros]` and every caller value under the arguments sits below the window.
    ///
    /// Only instructions are emitted: the padding exists between this and the matching
    /// [`Self::discard_call_window_padding`] only, so the emulated operand stack never models it.
    fn pad_call_window(&mut self, num_arg_felts: usize, span: SourceSpan) {
        use masm::Instruction as I;

        let num_pad_felts = CALL_WINDOW_FELTS - num_arg_felts;
        match num_arg_felts {
            16 => {}
            // Word-sized argument lists move as whole words: [z.., args] -> [args, z..]
            0 => self.emit_n(4, I::PadW, span),
            4 => {
                self.emit_n(3, I::PadW, span);
                self.emit(I::MovUpW3, span);
            }
            8 => {
                self.emit_n(2, I::PadW, span);
                self.emit(I::SwapDw, span);
            }
            12 => {
                self.emit(I::PadW, span);
                self.emit(I::MovDnW3, span);
            }
            // Few pads: sink each zero under the arguments right after pushing it
            n if n > 8 => {
                for _ in 0..num_pad_felts {
                    self.emit_push(Felt::ZERO, span);
                    self.emit(super::movdn_from_offset(n), span);
                }
            }
            // Few arguments: push all zeros, then raise the arguments from the window's bottom,
            // the last one first, so the first argument ends on top
            n => {
                self.emit_n(num_pad_felts / 4, I::PadW, span);
                for _ in 0..num_pad_felts % 4 {
                    self.emit_push(Felt::ZERO, span);
                }
                self.emit_n(n, I::MovUp15, span);
            }
        }
    }

    /// Discard the padding a `call`/`syscall`/`dyncall` callee leaves under its
    /// `num_result_felts` result elements, turning the returned window `[results, pad]` into
    /// `[results]` on top of the caller values that were below the window.
    ///
    /// Like [`Self::pad_call_window`], only instructions are emitted: the emulated operand stack
    /// already holds just the results there.
    fn discard_call_window_padding(&mut self, num_result_felts: usize, span: SourceSpan) {
        use masm::Instruction as I;

        let num_pad_felts = CALL_WINDOW_FELTS - num_result_felts;
        match num_result_felts {
            16 => {}
            // Word-sized result lists move as whole words: [results, p..] -> [p.., results]
            0 => self.emit_n(4, I::DropW, span),
            4 => {
                self.emit(I::MovDnW3, span);
                self.emit_n(3, I::DropW, span);
            }
            8 => {
                self.emit(I::SwapDw, span);
                self.emit_n(2, I::DropW, span);
            }
            12 => {
                self.emit(I::MovUpW3, span);
                self.emit(I::DropW, span);
            }
            // Little padding: raise and drop each pad element from under the results
            m if m > 8 => {
                for _ in 0..num_pad_felts {
                    self.emit(super::movup_from_offset(m), span);
                    self.emit(I::Drop, span);
                }
            }
            // Few results: sink them to the window's bottom, the first one first so they keep
            // their order, then drop the padding now on top
            m => {
                self.emit_n(m, I::MovDn15, span);
                self.emit_n(num_pad_felts / 4, I::DropW, span);
                self.emit_n(num_pad_felts % 4, I::Drop, span);
            }
        }
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
        let scratch = crate::linker::DYNCALL_ROOT_ADDR;
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
        assert_eq!(insts.len(), 32);
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
        // Pad the two argument felts to the 16-element call window
        assert_eq!(&insts[14..21], pad_call_window_insts(2).as_slice());
        // Push the scratch address, then the frame-traced dyncall pops it
        assert!(
            matches!(&insts[21], masm::Instruction::Push(masm::Immediate::Value(value)) if *value.inner() == scratch.into()),
            "expected push of the scratch address, got {:?}",
            insts[21]
        );
        assert!(matches!(&insts[22], masm::Instruction::EmitImm(_)));
        assert_eq!(insts[23], masm::Instruction::DynCall);
        assert!(matches!(&insts[24], masm::Instruction::EmitImm(_)));
        // Discard the padding under the one result felt
        assert_eq!(&insts[25..], discard_call_window_padding_insts(1).as_slice());
    }

    /// The instructions of `block`, which must contain nothing else.
    fn block_insts(block: &[Op]) -> Vec<masm::Instruction> {
        block
            .iter()
            .map(|op| match op {
                Op::Inst(inst) => inst.clone().into_inner(),
                op => panic!("unexpected non-instruction op: {op:?}"),
            })
            .collect()
    }

    /// The instructions [`OpEmitter::pad_call_window`] emits for `n` argument felts.
    fn pad_call_window_insts(n: usize) -> Vec<masm::Instruction> {
        let mut block = Vec::default();
        let mut stack = OperandStack::new(Rc::new(Context::default()));
        let mut invoked = BTreeSet::default();
        OpEmitter::new(&mut invoked, &mut block, &mut stack)
            .pad_call_window(n, SourceSpan::default());
        block_insts(&block)
    }

    /// The instructions [`OpEmitter::discard_call_window_padding`] emits for `m` result felts.
    fn discard_call_window_padding_insts(m: usize) -> Vec<masm::Instruction> {
        let mut block = Vec::default();
        let mut stack = OperandStack::new(Rc::new(Context::default()));
        let mut invoked = BTreeSet::default();
        OpEmitter::new(&mut invoked, &mut block, &mut stack)
            .discard_call_window_padding(m, SourceSpan::default());
        block_insts(&block)
    }

    /// A procedure path for the call tests.
    fn test_callee() -> masm::InvocationTarget {
        let name = masm::ProcedureName::new("callee").unwrap();
        let module = masm::LibraryPath::new("test").unwrap();
        let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
        masm::InvocationTarget::Path(masm::Span::new(SourceSpan::default(), qualified.into_inner()))
    }

    /// Run `insts` on a physical operand stack `stack` (top at index 0), handling a `call` the
    /// way the VM and a convention-following callee do: the callee must observe exactly
    /// `[args(n), zeros(16 - n)]`, and returns `[results(m), junk(16 - m)]`, where the results
    /// are `1000 + i` and the junk is `2000 + i`.
    fn run_call_sequence(
        insts: &[masm::Instruction],
        stack: &mut alloc::vec::Vec<u64>,
        args: &[u64],
        m: usize,
    ) {
        use masm::Instruction as I;

        use crate::emit::{movdn_from_offset, movup_from_offset};

        for inst in insts {
            match inst {
                I::EmitImm(_) => {}
                I::PadW => stack.splice(0..0, [0; 4]).for_each(drop),
                I::Push(masm::Immediate::Value(value)) => {
                    assert_eq!(*value.inner(), Felt::ZERO.into(), "only zeros are pushed");
                    stack.insert(0, 0);
                }
                I::Drop => {
                    stack.remove(0);
                }
                I::DropW => stack.drain(0..4).for_each(drop),
                I::SwapDw => {
                    let top: alloc::vec::Vec<u64> = stack.drain(0..8).collect();
                    stack.splice(8..8, top).for_each(drop);
                }
                I::MovUpW3 => {
                    let word: alloc::vec::Vec<u64> = stack.drain(12..16).collect();
                    stack.splice(0..0, word).for_each(drop);
                }
                I::MovDnW3 => {
                    let word: alloc::vec::Vec<u64> = stack.drain(0..4).collect();
                    stack.splice(12..12, word).for_each(drop);
                }
                I::Call(_) => {
                    let window: alloc::vec::Vec<u64> = stack.drain(0..16).collect();
                    assert_eq!(&window[..args.len()], args, "callee arguments");
                    assert!(
                        window[args.len()..].iter().all(|felt| *felt == 0),
                        "the window under the arguments must be zeros: {window:?}"
                    );
                    let returned =
                        (0..m as u64).map(|i| 1000 + i).chain((m as u64..16).map(|i| 2000 + i));
                    stack.splice(0..0, returned).for_each(drop);
                }
                inst => {
                    if let Some(i) = (2..16).find(|i| movup_from_offset(*i) == *inst) {
                        let felt = stack.remove(i);
                        stack.insert(0, felt);
                    } else if let Some(i) = (2..16).find(|i| movdn_from_offset(*i) == *inst) {
                        let felt = stack.remove(0);
                        stack.insert(i, felt);
                    } else {
                        panic!("unexpected instruction in a call sequence: {inst:?}");
                    }
                }
            }
        }
    }

    /// Every argument/result width leaves the callee exactly `[args, zeros]` and the caller
    /// exactly `[results, values below the arguments]`.
    #[test]
    fn call_pads_window_and_discards_padding_for_every_width() {
        use midenc_hir::CallConv;

        for n in 0..=16usize {
            for m in 0..=16usize {
                let mut block = Vec::default();
                let context = Rc::new(Context::default());
                let mut stack = OperandStack::new(context.clone());
                let mut invoked = BTreeSet::default();
                let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

                // Caller values the call must preserve, then the arguments
                emitter.push(Type::U64);
                emitter.push(Type::I1);
                for _ in 0..n {
                    emitter.push(Type::Felt);
                }
                let signature = Signature::with_convention(
                    &context,
                    CallConv::ComponentModel,
                    vec![Type::Felt; n],
                    vec![Type::Felt; m],
                );
                emitter.call(test_callee(), &signature, SourceSpan::default());

                // The model holds the results on top of the preserved caller values
                let model: alloc::vec::Vec<Type> = emitter
                    .stack()
                    .iter()
                    .rev()
                    .map(|operand| Type::try_from(operand).unwrap())
                    .collect();
                let expected_model: alloc::vec::Vec<Type> =
                    core::iter::repeat_n(Type::Felt, m).chain([Type::I1, Type::U64]).collect();
                assert_eq!(model, expected_model);

                // The physical stack matches it: args 1.., caller values 100.. (I1 then U64
                // limbs), and 20 elements of deeper stack 500..
                let args: alloc::vec::Vec<u64> = (1..=n as u64).collect();
                let below: alloc::vec::Vec<u64> = (100..103).chain(500..520).collect();
                let mut physical: alloc::vec::Vec<u64> =
                    args.iter().copied().chain(below.iter().copied()).collect();
                run_call_sequence(&block_insts(&block), &mut physical, &args, m);
                let expected: alloc::vec::Vec<u64> =
                    (0..m as u64).map(|i| 1000 + i).chain(below.iter().copied()).collect();
                assert_eq!(physical, expected, "stack after a call with {n} args, {m} results");
            }
        }
    }

    /// Pin the call sequence for two argument and two result felts: zeros pushed and the
    /// arguments raised over them, then the results sunk to the window's bottom and the padding
    /// dropped.
    #[test]
    fn call_with_two_args_and_two_results_emits_padding_sequence() {
        use masm::Instruction as I;
        use midenc_hir::CallConv;

        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        emitter.push(Type::U64);
        emitter.push(Type::Felt);
        emitter.push(Type::U32);
        let signature = Signature::with_convention(
            &context,
            CallConv::ComponentModel,
            [Type::U32, Type::Felt],
            [Type::Felt, Type::U32],
        );
        emitter.call(test_callee(), &signature, SourceSpan::default());

        assert_eq!(emitter.stack_len(), 3);
        assert_eq!(emitter.stack()[0], Type::Felt);
        assert_eq!(emitter.stack()[1], Type::U32);
        assert_eq!(emitter.stack()[2], Type::U64);

        let zero = I::Push(masm::Immediate::Value(masm::Span::new(
            SourceSpan::default(),
            Felt::ZERO.into(),
        )));
        let insts = block_insts(&block);
        assert_eq!(insts.len(), 17);
        assert_eq!(
            &insts[..7],
            &[I::PadW, I::PadW, I::PadW, zero.clone(), zero, I::MovUp15, I::MovUp15]
        );
        assert!(matches!(&insts[7], I::EmitImm(_)));
        assert_eq!(insts[8], I::Call(test_callee()));
        assert!(matches!(&insts[9], I::EmitImm(_)));
        assert_eq!(
            &insts[10..],
            &[I::MovDn15, I::MovDn15, I::DropW, I::DropW, I::DropW, I::Drop, I::Drop]
        );
    }

    /// No arguments and no results: the window is all zeros going in and is dropped coming out.
    #[test]
    fn call_without_args_or_results_pads_and_drops_whole_window() {
        use masm::Instruction as I;

        assert_eq!(pad_call_window_insts(0), [I::PadW, I::PadW, I::PadW, I::PadW]);
        assert_eq!(discard_call_window_padding_insts(0), [I::DropW, I::DropW, I::DropW, I::DropW]);
    }

    /// A full window needs neither padding nor dropping.
    #[test]
    fn call_with_full_window_emits_no_padding() {
        assert!(pad_call_window_insts(16).is_empty());
        assert!(discard_call_window_padding_insts(16).is_empty());
    }

    /// A syscall pads and discards exactly like a call.
    #[test]
    fn syscall_pads_window_and_discards_padding() {
        use midenc_hir::CallConv;

        let mut block = Vec::default();
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context.clone());
        let mut invoked = BTreeSet::default();
        let mut emitter = OpEmitter::new(&mut invoked, &mut block, &mut stack);

        emitter.push(Type::Felt);
        let signature =
            Signature::with_convention(&context, CallConv::Wasm, [Type::Felt], [Type::Felt]);
        emitter.syscall(test_callee(), &signature, SourceSpan::default());

        let insts = block_insts(&block);
        let pad = pad_call_window_insts(1);
        let discard = discard_call_window_padding_insts(1);
        assert_eq!(&insts[..pad.len()], pad.as_slice());
        assert_eq!(insts[pad.len() + 1], masm::Instruction::SysCall(test_callee()));
        assert_eq!(&insts[pad.len() + 3..], discard.as_slice());
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
