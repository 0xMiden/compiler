use miden_assembly_syntax::parser::WordValue;
use midenc_hir::{
    self as hir, ArgumentExtension, ArgumentPurpose, Felt, FieldElement, Immediate, SourceSpan,
    Type,
};

use super::{OpEmitter, int64, masm};
use crate::TraceEvent;

impl OpEmitter<'_> {
    /// Assert that an integer value on the stack has the value 1
    ///
    /// This operation consumes the input value.
    pub fn assert(&mut self, _code: Option<u32>, span: SourceSpan) {
        let arg = self.stack.pop().expect("operand stack is empty");
        match arg.ty() {
            Type::Felt
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::U8
            | Type::I8
            | Type::I1 => {
                self.emit(masm::Instruction::Assert, span);
            }
            Type::I128 | Type::U128 => {
                self.emit_all(
                    [
                        masm::Instruction::Push(masm::Immediate::Value(masm::Span::new(
                            span,
                            WordValue([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]).into(),
                        ))),
                        masm::Instruction::AssertEqw,
                    ],
                    span,
                );
            }
            Type::U64 | Type::I64 => {
                self.emit_all([masm::Instruction::Assertz, masm::Instruction::Assert], span);
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
    pub fn assertz(&mut self, _code: Option<u32>, span: SourceSpan) {
        let arg = self.stack.pop().expect("operand stack is empty");
        match arg.ty() {
            Type::Felt
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::U8
            | Type::I8
            | Type::I1 => {
                self.emit(masm::Instruction::Assertz, span);
            }
            Type::U64 | Type::I64 => {
                self.emit_all([masm::Instruction::Assertz, masm::Instruction::Assertz], span);
            }
            Type::U128 | Type::I128 => {
                self.emit_all(
                    [
                        masm::Instruction::Push(masm::Immediate::Value(masm::Span::new(
                            span,
                            WordValue([Felt::ZERO; 4]).into(),
                        ))),
                        masm::Instruction::AssertEqw,
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
    pub fn assert_eq(&mut self, span: SourceSpan) {
        let rhs = self.pop().expect("operand stack is empty");
        let lhs = self.pop().expect("operand stack is empty");
        let ty = lhs.ty();
        assert_eq!(ty, rhs.ty(), "expected assert_eq operands to have the same type");
        match ty {
            Type::Felt
            | Type::U32
            | Type::I32
            | Type::U16
            | Type::I16
            | Type::U8
            | Type::I8
            | Type::I1 => {
                self.emit(masm::Instruction::AssertEq, span);
            }
            Type::U128 | Type::I128 => self.emit(masm::Instruction::AssertEqw, span),
            Type::U64 | Type::I64 => {
                self.emit_all(
                    [
                        // compare the hi bits
                        masm::Instruction::MovUp2,
                        masm::Instruction::AssertEq,
                        // compare the low bits
                        masm::Instruction::AssertEq,
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
        let ty = lhs.ty();
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
                        masm::Instruction::Assert,
                    ],
                    span,
                );
            }
            Type::I128 | Type::U128 => {
                self.push_immediate(imm, span);
                self.emit(masm::Instruction::AssertEqw, span)
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
                        masm::Instruction::EqImm(Felt::new(hi as u64).into()),
                        masm::Instruction::Assert,
                        masm::Instruction::EqImm(Felt::new(lo as u64).into()),
                        masm::Instruction::Assert,
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

    /// Execute the given procedure.
    ///
    /// A function called using this operation is invoked in the same memory context as the caller.
    pub fn exec(
        &mut self,
        callee: masm::InvocationTarget,
        signature: &hir::Signature,
        span: SourceSpan,
    ) {
        self.process_call_signature(&callee, signature, span);

        self.emit(masm::Instruction::Trace(TraceEvent::FrameStart.as_u32().into()), span);
        self.emit(masm::Instruction::Nop, span);
        self.emit(masm::Instruction::Exec(callee), span);
        self.emit(masm::Instruction::Trace(TraceEvent::FrameEnd.as_u32().into()), span);
        self.emit(masm::Instruction::Nop, span);
    }

    /// Execute the given procedure in a new context.
    ///
    /// A function called using this operation is invoked in a new memory context.
    pub fn call(
        &mut self,
        callee: masm::InvocationTarget,
        signature: &hir::Signature,
        span: SourceSpan,
    ) {
        self.process_call_signature(&callee, signature, span);

        self.emit(masm::Instruction::Trace(TraceEvent::FrameStart.as_u32().into()), span);
        self.emit(masm::Instruction::Nop, span);
        self.emit(masm::Instruction::Call(callee), span);
        self.emit(masm::Instruction::Trace(TraceEvent::FrameEnd.as_u32().into()), span);
        self.emit(masm::Instruction::Nop, span);
    }

    fn process_call_signature(
        &mut self,
        callee: &masm::InvocationTarget,
        signature: &hir::Signature,
        span: SourceSpan,
    ) {
        for i in 0..signature.arity() {
            let param = &signature.params[i];
            let arg = self.stack.pop().expect("operand stack is empty");
            let ty = arg.ty();
            // Validate the purpose matches
            match param.purpose {
                ArgumentPurpose::StructReturn => {
                    assert_eq!(
                        i, 0,
                        "invalid function signature: sret parameters must be the first parameter, \
                         and only one sret parameter is allowed"
                    );
                    assert_eq!(
                        signature.results.len(),
                        0,
                        "invalid function signature: a function with sret parameters cannot also \
                         have results"
                    );
                    assert!(
                        ty.is_pointer(),
                        "invalid exec to {callee}: invalid argument for sret parameter, expected \
                         {}, got {ty}",
                        &param.ty
                    );
                }
                ArgumentPurpose::Default => (),
            }
            // Validate that the argument type is valid for the parameter ABI
            match param.extension {
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
