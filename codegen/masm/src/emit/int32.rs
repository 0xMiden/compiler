use miden_core::{Felt, FieldElement};
use midenc_hir::{Overflow, SourceSpan};

use super::{OpEmitter, dup_from_offset, felt, masm, movup_from_offset};

pub const SIGN_BIT: u32 = 1 << 31;

#[allow(unused)]
impl OpEmitter<'_> {
    /// Emits code to apply a constant 32-bit mask, `mask`, to a u32 value on top of the stack.
    ///
    /// The value on top of the stack IS consumed.
    ///
    /// NOTE: This function does not validate that the value on top of the stack is
    /// a valid u32 - the caller is responsible for such validation.
    ///
    /// # Stack Effects
    ///
    /// `[a, ..] => [a & mask, ..]`
    #[inline]
    pub fn const_mask_u32(&mut self, mask: u32, span: SourceSpan) {
        self.emit_push(mask, span);
        self.emit(masm::Instruction::U32And, span);
    }

    /// Emits code to apply a 32-bit mask, `mask`, to a u32 value, `input`.
    ///
    /// Both `mask` and `input` are operands on the stack, with `mask` on top.
    ///
    /// While `mask` is consumed by this operation, `input` IS NOT consumed.
    ///
    /// NOTE: This function assumes that the caller has validated that both values are valid u32.
    ///
    /// # Stack Effects
    ///
    /// `[mask, input, ..] => [input & mask, input]`
    #[inline]
    pub fn mask_u32(&mut self, span: SourceSpan) {
        self.emit_all([masm::Instruction::Dup1, masm::Instruction::U32And], span);
    }

    /// Emits code to check if all bits of `flags` are set in the u32 value on top of the stack.
    ///
    /// The value on top of the stack IS NOT consumed.
    ///
    /// NOTE: This function does not validate that the value on top of the stack is
    /// a valid u32 - the caller is responsible for such validation.
    ///
    /// # Stack Effects
    ///
    /// `[a, ..] => [a & flags == flags, a]`
    #[inline]
    pub fn is_const_flag_set_u32(&mut self, flags: u32, span: SourceSpan) {
        self.emit(masm::Instruction::Dup0, span);
        self.const_mask_u32(flags, span);
        self.emit(masm::Instruction::EqImm(Felt::new(flags as u64).into()), span);
    }

    /// Emits code to check if all bits of `mask` are set in `input`.
    ///
    /// Both `mask` and `input` are operands on the stack, with `mask` on top.
    ///
    /// While `mask` is consumed by this operation, `input` IS NOT consumed.
    ///
    /// NOTE: This function assumes that the caller has validated that both values are valid u32.
    ///
    /// # Stack Effects
    ///
    /// `[mask, input, ..] => [input & mask == mask, input]`
    #[inline]
    pub fn is_flag_set_u32(&mut self, span: SourceSpan) {
        self.emit_all(
            [
                masm::Instruction::Dup1,   // [input, mask, input]
                masm::Instruction::Dup1,   // [mask, input, mask, input]
                masm::Instruction::U32And, // [input & mask, mask, input]
                masm::Instruction::Eq,     // [input & mask == mask, input]
            ],
            span,
        );
    }

    /// Check if a 32-bit integer value on the operand stack has its sign bit set.
    ///
    /// The value on top of the stack IS NOT consumed.
    ///
    /// See `is_const_flag_set` for semantics and stack effects.
    #[inline]
    pub fn is_signed_int32(&mut self, span: SourceSpan) {
        self.is_const_flag_set_u32(SIGN_BIT, span);
    }

    /// Check if a 32-bit integer value on the operand stack does not have its sign bit set.
    ///
    /// The value on top of the stack IS NOT consumed.
    #[inline(always)]
    pub fn is_unsigned_int32(&mut self, span: SourceSpan) {
        self.is_signed_int32(span);
        self.emit(masm::Instruction::Not, span);
    }

    /// Emits code to assert that a 32-bit value on the operand stack has the i32 sign bit set.
    ///
    /// The value on top of the stack IS NOT consumed.
    ///
    /// See `is_signed` for semantics and stack effects of the signedness check.
    #[inline]
    pub fn assert_signed_int32(&mut self, span: SourceSpan) {
        self.is_signed_int32(span);
        self.emit(masm::Instruction::Assert, span);
    }

    /// Emits code to assert that a 32-bit value on the operand stack does not have the i32 sign bit
    /// set.
    ///
    /// The value on top of the stack IS NOT consumed.
    ///
    /// See `is_signed` for semantics and stack effects of the signedness check.
    #[inline]
    pub fn assert_unsigned_int32(&mut self, span: SourceSpan) {
        self.is_signed_int32(span);
        self.emit(masm::Instruction::Assertz, span);
    }

    /// Assert that the 32-bit value on the stack is a valid i32 value
    pub fn assert_i32(&mut self, span: SourceSpan) {
        // Copy the value on top of the stack
        self.emit(masm::Instruction::Dup0, span);
        // Assert the value does not overflow i32::MAX or underflow i32::MIN
        // This can be checked by validating that when interpreted as a u32,
        // the value is <= i32::MIN, which is 1 more than i32::MAX.
        self.push_i32(i32::MIN, span);
        self.emit(masm::Instruction::U32Lte, span);
        self.emit(masm::Instruction::Assert, span);
    }

    /// Emits code to assert that a 32-bit value on the operand stack is equal to the given constant
    /// value.
    ///
    /// The value on top of the stack IS NOT consumed.
    ///
    /// # Stack Effects
    ///
    /// `[input, ..] => [input, ..]`
    #[inline]
    pub fn assert_eq_imm_u32(&mut self, value: u32, span: SourceSpan) {
        self.emit_all(
            [
                masm::Instruction::Dup0,
                masm::Instruction::EqImm(Felt::new(value as u64).into()),
                masm::Instruction::Assert,
            ],
            span,
        );
    }

    /// Emits code to assert that two 32-bit values, `expected` and `value`, on top of the operand
    /// stack are equal, without consuming `value`.
    ///
    /// The `expected` operand is consumed, while the `value` operand IS NOT.
    ///
    /// # Stack Effects
    ///
    /// `[expected, input, ..] => [input, ..]`
    #[inline]
    pub fn assert_eq_u32(&mut self, span: SourceSpan) {
        self.emit_all([masm::Instruction::Dup1, masm::Instruction::AssertEq], span);
    }

    /// Emits code to select a constant u32 value, using the `n`th value on the operand
    /// stack as the condition for the select.
    ///
    /// This function pushes `b` then `a` on the stack, moves the `n`th value to the top
    /// of the stack, and then executes a conditional drop. This has the effect of consuming
    /// all three operands, placing only a single value back on the operand stack; the
    /// selected value, either `a` or `b`. Use `dup_select` if you would rather copy
    /// the conditional rather than move it.
    pub fn mov_select_int32(&mut self, a: u32, b: u32, n: u8, span: SourceSpan) {
        assert_valid_stack_index!(n);
        // If the value we need will get pushed off the end of the stack,
        // bring it closer first, and adjust our `n` accordingly
        if n > 13 {
            self.emit(movup_from_offset(n as usize), span);
            self.select_int32(a, b, span);
        } else {
            self.emit_push(b, span);
            self.emit_push(a, span);
            self.emit_all([movup_from_offset(n as usize + 2), masm::Instruction::CDrop], span);
        }
    }

    /// Same semantics as `mov_select`, but copies the `n`th value on the operand
    /// stack rather than moving it.
    ///
    /// # Stack Effects
    ///
    /// Moves `c` to the top of the stack, where `c` is the `n`th value on the operand stack,
    /// then applies `select`.
    pub fn dup_select_int32(&mut self, a: u32, b: u32, n: u8, span: SourceSpan) {
        assert_valid_stack_index!(n);
        // If the value we need will get pushed off the end of the stack,
        // bring it closer first, and adjust our `n` accordingly
        if n > 13 {
            self.emit(dup_from_offset(n as usize), span);
            self.select_int32(a, b, span);
        } else {
            self.emit_push(b, span);
            self.emit_push(a, span);
            self.emit_all([dup_from_offset(n as usize + 2), masm::Instruction::CDrop], span);
        }
    }

    /// Emits code to select between two u32 constants, given a boolean value on top of the stack
    ///
    /// # Stack Effects
    ///
    /// `[c, a, b, ..] => [d, ..] where d is c == 1 ? a : b`
    pub fn select_int32(&mut self, a: u32, b: u32, span: SourceSpan) {
        self.emit_push(b, span);
        self.emit_push(a, span);
        self.emit_all([masm::Instruction::MovUp2, masm::Instruction::CDrop], span);
    }

    /// Convert an i32/u32 value on the stack to a signed N-bit integer value
    ///
    /// Execution traps if the value cannot fit in the signed N-bit range.
    pub fn int32_to_int(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        // Push is_signed on the stack
        self.is_signed_int32(span);
        // Pop the is_signed flag, and replace it with a selected mask
        // for the upper reserved bits of the N-bit range
        let reserved = 32 - n;
        // Add one bit to the reserved bits to represent the sign bit,
        // and subtract it from the shift to account for the loss
        let mask = (2u32.pow(reserved + 1) - 1) << (n - 1);
        self.select_int32(mask, 0, span);
        self.emit_all(
            [
                // Copy the input to the top of the stack for the masking op
                masm::Instruction::Dup1,
                // Copy the mask value for the masking op
                masm::Instruction::Dup1,
                // Apply the mask
                masm::Instruction::U32And,
                // Assert that the masked bits and the mask are equal
                masm::Instruction::AssertEq,
            ],
            span,
        );
    }

    /// Convert an i32/u32 value on the stack to a signed N-bit integer value
    ///
    /// Places a boolean on top of the stack indicating if the conversion was successful
    pub fn try_int32_to_int(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        // Push is_signed on the stack
        self.is_signed_int32(span);
        // Pop the is_signed flag, and replace it with a selected mask
        // for the upper reserved bits of the N-bit range
        let reserved = 32 - n;
        // Add one bit to the reserved bits to represent the sign bit,
        // and subtract it from the shift to account for the loss
        let mask = (2u32.pow(reserved + 1) - 1) << (n - 1);
        self.select_int32(mask, 0, span);
        self.emit_all(
            [
                // Copy the input to the top of the stack for the masking op
                masm::Instruction::Dup1,
                // Copy the mask value for the masking op
                masm::Instruction::Dup1,
                // Apply the mask
                masm::Instruction::U32And,
                // Assert that the masked bits and the mask are equal
                masm::Instruction::Eq,
            ],
            span,
        );
    }

    /// Convert an i32/u32 value on the stack to an unsigned N-bit integer value
    ///
    /// Execution traps if the value cannot fit in the unsigned N-bit range.
    pub fn int32_to_uint(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        // Mask the value and ensure that the unused bits above the N-bit range are 0
        let reserved = 32 - n;
        let mask = (2u32.pow(reserved) - 1) << n;
        // Copy the input
        self.emit(masm::Instruction::Dup1, span);
        // Apply the mask
        self.emit_push(mask, span);
        self.emit(masm::Instruction::U32And, span);
        // Assert the masked value is all 0s
        self.emit(masm::Instruction::Assertz, span);
    }

    /// Convert an i32/u32 value on the stack to an unsigned N-bit integer value
    ///
    /// Places a boolean on top of the stack indicating if the conversion was successful
    pub fn try_int32_to_uint(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        // Mask the value and ensure that the unused bits above the N-bit range are 0
        let reserved = 32 - n;
        let mask = (2u32.pow(reserved) - 1) << n;
        // Copy the input
        self.emit(masm::Instruction::Dup1, span);
        // Apply the mask
        self.emit_push(mask, span);
        self.emit(masm::Instruction::U32And, span);
        // Assert the masked value is all 0s
        self.emit(masm::Instruction::EqImm(Felt::ZERO.into()), span);
    }

    /// Emit code to truncate a 32-bit value on top of the operand stack, to N bits, where N is <=
    /// 32
    ///
    /// This consumes the input value, and leaves an N-bit value on the stack.
    ///
    /// NOTE: This function does not validate the input as < 2^32, the caller is expected to
    /// validate this.
    #[inline]
    pub fn trunc_int32(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        // Mask out any bits between N and 32.
        let unused_bits = 32 - n;
        if unused_bits > 0 {
            self.const_mask_u32((1 << (32 - unused_bits)) - 1, span);
        }
    }

    /// Emit code to zero-extend a 32-bit value to N bits, where N <= 128
    ///
    /// This operation assumes all N-bit integers greater than 32 bits use 32-bit limbs.
    ///
    /// NOTE: This operation does not check the sign bit, it is assumed the value is
    /// either an unsigned integer, or a non-negative signed integer.
    #[inline]
    pub fn zext_int32(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 32);
        // Only values larger than 32 bits require padding
        if n <= 32 {
            return;
        }
        let num_bits = n % 32;
        let num_elements = (n / 32) + (num_bits > 0) as u32;
        let needed = num_elements - 1;
        self.emit_n(
            needed as usize,
            masm::Instruction::Push(masm::Immediate::Value(masm::Span::new(
                span,
                Felt::ZERO.into(),
            ))),
            span,
        );
    }

    /// Emit code to sign-extend a signed 32-bit value to N bits, where N <= 128
    ///
    /// This operation assumes all N-bit integers greater than 32 bits use 32-bit limbs.
    ///
    /// NOTE: This operation treats the most significant bit as the sign bit, it is
    /// assumed the value is an i32, it is up to the caller to ensure this is a valid
    /// operation to perform on the input.
    #[inline]
    pub fn sext_int32(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 32);
        self.is_signed_int32(span);
        self.select_int32(u32::MAX, 0, span);
        self.pad_int32(n, span);
    }

    /// Emit code to pad a 32-bit value out to N bits, where N >= 32.
    ///
    /// N must be a power of two.
    ///
    /// The padding value is expected on top of the stack, followed by the 32-bit value to pad.
    ///
    /// This operation assumes all N-bit integers greater than 32 bits use 32-bit limbs.
    ///
    /// The padding value will be duplicated for each additional 32-bit limb needed to
    /// ensure that there are enough limbs on the stack to represent an N-bit integer.
    #[inline]
    pub fn pad_int32(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 32);
        // We need one element for each 32-bit limb
        let num_elements = n / 32;
        // We already have the input u32, as well as the pad value, so deduct
        // those elements from the number needed.
        let needed = num_elements.saturating_sub(2);
        self.emit_n(needed as usize, masm::Instruction::Dup0, span);
    }

    /// Push a u32 value on the stack
    #[inline(always)]
    pub fn push_u32(&mut self, i: u32, span: SourceSpan) {
        self.emit_push(i, span);
    }

    /// Push a i32 value on the stack
    #[inline(always)]
    pub fn push_i32(&mut self, i: i32, span: SourceSpan) {
        self.emit_push(i as u32, span);
    }

    /// This is the inverse operation of the Miden VM `u32split` instruction.
    ///
    /// This takes two 32-bit limbs, and produces a felt.
    ///
    /// NOTE: It is expected that the caller has validated that the limbs are valid u32 values.
    pub fn u32unsplit(&mut self, span: SourceSpan) {
        self.emit_all(
            [
                masm::Instruction::MulImm(felt::U32_FIELD_MODULUS.into()),
                masm::Instruction::Add,
            ],
            span,
        );
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a + b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    #[inline(always)]
    pub fn add_u32(&mut self, overflow: Overflow, span: SourceSpan) {
        self.emit(
            match overflow {
                Overflow::Unchecked => masm::Instruction::Add,
                Overflow::Checked => {
                    return self
                        .emit_all([masm::Instruction::Add, masm::Instruction::U32Assert], span);
                }
                Overflow::Wrapping => masm::Instruction::U32WrappingAdd,
                Overflow::Overflowing => masm::Instruction::U32OverflowingAdd,
            },
            span,
        );
    }

    /// Pops two i32 values off the stack, `b` and `a`, and performs `a + b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    #[inline(always)]
    pub fn add_i32(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => {
                self.emit(masm::Instruction::U32WrappingAdd, span)
            }
            Overflow::Checked => self.raw_exec("intrinsics::i32::checked_add", span),
            Overflow::Overflowing => self.raw_exec("intrinsics::i32::overflowing_add", span),
        }
    }

    /// Pops a u32 value off the stack, `a`, and performs `a + <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Adding zero is a no-op.
    #[inline]
    pub fn add_imm_u32(&mut self, imm: u32, overflow: Overflow, span: SourceSpan) {
        if imm == 0 {
            return;
        }
        self.emit(
            match overflow {
                Overflow::Unchecked if imm == 1 => masm::Instruction::AddImm(Felt::ONE.into()),
                Overflow::Unchecked => masm::Instruction::AddImm(Felt::new(imm as u64).into()),
                Overflow::Checked => {
                    return self.emit_all(
                        [
                            masm::Instruction::AddImm(Felt::new(imm as u64).into()),
                            masm::Instruction::U32Assert,
                        ],
                        span,
                    );
                }
                Overflow::Wrapping => masm::Instruction::U32WrappingAddImm(imm.into()),
                Overflow::Overflowing => masm::Instruction::U32OverflowingAddImm(imm.into()),
            },
            span,
        );
    }

    /// Pops a i32 value off the stack, `a`, and performs `a + <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Adding zero is a no-op.
    #[inline]
    pub fn add_imm_i32(&mut self, imm: i32, overflow: Overflow, span: SourceSpan) {
        if imm == 0 {
            return;
        }
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => {
                self.add_imm_u32(imm as u32, overflow, span)
            }
            Overflow::Checked => {
                self.emit_push(imm as u32, span);
                self.raw_exec("intrinsics::i32::checked_add", span);
            }
            Overflow::Overflowing => {
                self.emit_push(imm as u32, span);
                self.raw_exec("intrinsics::i32::overflowing_add", span);
            }
        }
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a - b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    pub fn sub_u32(&mut self, overflow: Overflow, span: SourceSpan) {
        self.emit(
            match overflow {
                Overflow::Unchecked => masm::Instruction::Sub,
                Overflow::Checked => {
                    return self
                        .emit_all([masm::Instruction::Sub, masm::Instruction::U32Assert], span);
                }
                Overflow::Wrapping => masm::Instruction::U32WrappingSub,
                Overflow::Overflowing => masm::Instruction::U32OverflowingSub,
            },
            span,
        );
    }

    /// Pops two i32 values off the stack, `b` and `a`, and performs `a - b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    pub fn sub_i32(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => self.sub_u32(overflow, span),
            Overflow::Checked => self.raw_exec("intrinsics::i32::checked_sub", span),
            Overflow::Overflowing => self.raw_exec("intrinsics::i32::overflowing_sub", span),
        }
    }

    /// Pops a u32 value off the stack, `a`, and performs `a - <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Subtracting zero is a no-op.
    #[inline]
    pub fn sub_imm_u32(&mut self, imm: u32, overflow: Overflow, span: SourceSpan) {
        if imm == 0 {
            return;
        }
        self.emit(
            match overflow {
                Overflow::Unchecked => masm::Instruction::SubImm(Felt::new(imm as u64).into()),
                Overflow::Checked => {
                    return self.emit_all(
                        [
                            masm::Instruction::SubImm(Felt::new(imm as u64).into()),
                            masm::Instruction::U32Assert,
                        ],
                        span,
                    );
                }
                Overflow::Wrapping => masm::Instruction::U32WrappingSubImm(imm.into()),
                Overflow::Overflowing => masm::Instruction::U32OverflowingSubImm(imm.into()),
            },
            span,
        );
    }

    /// Pops a i32 value off the stack, `a`, and performs `a - <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Subtracting zero is a no-op.
    #[inline]
    pub fn sub_imm_i32(&mut self, imm: i32, overflow: Overflow, span: SourceSpan) {
        if imm == 0 {
            return;
        }
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => {
                self.sub_imm_u32(imm as u32, overflow, span)
            }
            Overflow::Checked => {
                self.emit_push(imm as u32, span);
                self.raw_exec("intrinsics::i32::checked_sub", span);
            }
            Overflow::Overflowing => {
                self.emit_push(imm as u32, span);
                self.raw_exec("intrinsics::i32::overflowing_sub", span);
            }
        }
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a * b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    pub fn mul_u32(&mut self, overflow: Overflow, span: SourceSpan) {
        self.emit(
            match overflow {
                Overflow::Unchecked => masm::Instruction::Mul,
                Overflow::Checked => {
                    return self
                        .emit_all([masm::Instruction::Mul, masm::Instruction::U32Assert], span);
                }
                Overflow::Wrapping => masm::Instruction::U32WrappingMul,
                Overflow::Overflowing => masm::Instruction::U32OverflowingMul,
            },
            span,
        );
    }

    /// Pops two i32 values off the stack, `b` and `a`, and performs `a * b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    pub fn mul_i32(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => {
                self.raw_exec("intrinsics::i32::wrapping_mul", span)
            }
            Overflow::Checked => self.raw_exec("intrinsics::i32::checked_mul", span),
            Overflow::Overflowing => self.raw_exec("intrinsics::i32::overflowing_mul", span),
        }
    }

    /// Pops a u32 value off the stack, `a`, and performs `a * <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Multiplying by zero is transformed into a sequence which drops the input value
    /// and pushes a constant zero on the stack.
    ///
    /// Multiplying by one is a no-op.
    #[inline]
    pub fn mul_imm_u32(&mut self, imm: u32, overflow: Overflow, span: SourceSpan) {
        match imm {
            0 => {
                self.emit(masm::Instruction::Drop, span);
                self.emit_push(0u32, span);
            }
            1 => (),
            imm => {
                self.emit(
                    match overflow {
                        Overflow::Unchecked => {
                            masm::Instruction::MulImm(Felt::new(imm as u64).into())
                        }
                        Overflow::Checked => {
                            return self.emit_all(
                                [
                                    masm::Instruction::MulImm(Felt::new(imm as u64).into()),
                                    masm::Instruction::U32Assert,
                                ],
                                span,
                            );
                        }
                        Overflow::Wrapping => masm::Instruction::U32WrappingMulImm(imm.into()),
                        Overflow::Overflowing => {
                            masm::Instruction::U32OverflowingMulImm(imm.into())
                        }
                    },
                    span,
                );
            }
        }
    }

    /// Pops a i32 value off the stack, `a`, and performs `a * <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Multiplying by zero is transformed into a sequence which drops the input value
    /// and pushes a constant zero on the stack.
    ///
    /// Multiplying by one is a no-op.
    #[inline]
    pub fn mul_imm_i32(&mut self, imm: i32, overflow: Overflow, span: SourceSpan) {
        match imm {
            0 => {
                self.emit(masm::Instruction::Drop, span);
                self.emit_push(0u32, span);
            }
            1 => (),
            imm => match overflow {
                Overflow::Unchecked | Overflow::Wrapping => {
                    self.emit_push(imm as u32, span);
                    self.raw_exec("intrinsics::i32::wrapping_mul", span);
                }
                Overflow::Checked => {
                    self.emit_push(imm as u32, span);
                    self.raw_exec("intrinsics::i32::checked_mul", span)
                }
                Overflow::Overflowing => {
                    self.emit_push(imm as u32, span);
                    self.raw_exec("intrinsics::i32::overflowing_mul", span);
                }
            },
        }
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a / b`.
    ///
    /// This operation is checked, so if the operands or result are not valid u32, execution traps.
    pub fn checked_div_u32(&mut self, span: SourceSpan) {
        self.emit_all([masm::Instruction::U32Div, masm::Instruction::U32Assert], span);
    }

    /// Pops two i32 values off the stack, `b` and `a`, and performs `a / b`.
    ///
    /// This operation is checked, so if the operands or result are not valid i32, execution traps.
    pub fn checked_div_i32(&mut self, span: SourceSpan) {
        self.raw_exec("intrinsics::i32::checked_div", span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a / <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    ///
    /// This operation is checked, so if the operand or result are not valid u32, execution traps.
    pub fn checked_div_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.emit_all(
            [masm::Instruction::U32DivImm(imm.into()), masm::Instruction::U32Assert],
            span,
        );
    }

    /// Pops a i32 value off the stack, `a`, and performs `a / <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    ///
    /// This operation is checked, so if the operand or result are not valid i32, execution traps.
    pub fn checked_div_imm_i32(&mut self, imm: i32, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.emit_push(imm as u32, span);
        self.raw_exec("intrinsics::i32::checked_div", span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a / b`.
    ///
    /// This operation is unchecked, so the result is not guaranteed to be a valid u32
    pub fn unchecked_div_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Div, span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a / <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    pub fn unchecked_div_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.emit(masm::Instruction::U32DivImm(imm.into()), span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a % b`.
    ///
    /// This operation is checked, so if the operands or result are not valid u32, execution traps.
    pub fn checked_mod_u32(&mut self, span: SourceSpan) {
        self.emit_all([masm::Instruction::U32Mod, masm::Instruction::U32Assert], span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a % <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    ///
    /// This operation is checked, so if the operand or result are not valid u32, execution traps.
    pub fn checked_mod_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.emit_all(
            [masm::Instruction::U32ModImm(imm.into()), masm::Instruction::U32Assert],
            span,
        );
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a % b`.
    ///
    /// This operation is unchecked, so the result is not guaranteed to be a valid u32
    pub fn unchecked_mod_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Mod, span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a % <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    pub fn unchecked_mod_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.emit(masm::Instruction::U32ModImm(imm.into()), span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and pushes `a / b`, then `a % b` on the
    /// stack.
    ///
    /// This operation is checked, so if the operands or result are not valid u32, execution traps.
    pub fn checked_divmod_u32(&mut self, span: SourceSpan) {
        self.emit_all([masm::Instruction::U32DivMod, masm::Instruction::U32Assert], span);
    }

    /// Pops a u32 value off the stack, `a`, and pushes `a / <imm>`, then `a % <imm>` on the stack.
    ///
    /// This operation is checked, so if the operands or result are not valid u32, execution traps.
    pub fn checked_divmod_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.emit_all(
            [masm::Instruction::U32DivModImm(imm.into()), masm::Instruction::U32Assert],
            span,
        );
    }

    /// Pops two u32 values off the stack, `b` and `a`, and pushes `a / b`, then `a % b` on the
    /// stack.
    ///
    /// This operation is unchecked, so the result is not guaranteed to be a valid u32
    pub fn unchecked_divmod_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32DivMod, span);
    }

    /// Pops a u32 value off the stack, `a`, and pushes `a / <imm>`, then `a % <imm>` on the stack.
    ///
    /// This operation is unchecked, so the result is not guaranteed to be a valid u32
    pub fn unchecked_divmod_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.emit(masm::Instruction::U32DivModImm(imm.into()), span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a & b`
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn band_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32And, span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a & <imm>`
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn band_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        self.emit_push(imm, span);
        self.emit(masm::Instruction::U32And, span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a | b`
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn bor_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Or, span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a | <imm>`
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn bor_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        self.emit_push(imm, span);
        self.emit(masm::Instruction::U32Or, span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a ^ b`
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn bxor_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Xor, span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a ^ <imm>`
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn bxor_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        self.emit_push(imm, span);
        self.emit(masm::Instruction::U32Xor, span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `!a`
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn bnot_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32WrappingSubImm((-1i32 as u32).into()), span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a << b`
    ///
    /// Execution traps if `b` > 31.
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn shl_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Shl, span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a << <imm>`
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn shl_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert!(imm < 32, "invalid shift value: must be < 32, got {imm}");
        self.emit(masm::Instruction::U32ShlImm((imm as u8).into()), span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a >> b`
    ///
    /// Execution traps if `b` > 31.
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn shr_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Shr, span);
    }

    /// Pops two i32 values off the stack, `b` and `a`, and performs `a >> b`
    ///
    /// Execution traps if `b` > 31.
    ///
    /// This operation is checked, if the operands or result are not valid i32, execution traps.
    pub fn shr_i32(&mut self, span: SourceSpan) {
        self.raw_exec("intrinsics::i32::checked_shr", span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a >> <imm>`
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn shr_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert!(imm < 32, "invalid shift value: must be < 32, got {imm}");
        self.emit(masm::Instruction::U32ShrImm((imm as u8).into()), span);
    }

    /// Pops a i32 value off the stack, `a`, and performs `a >> <imm>`
    ///
    /// This operation is checked, if the operand or result are not valid i32, execution traps.
    pub fn shr_imm_i32(&mut self, imm: u32, span: SourceSpan) {
        assert!(imm < 32, "invalid shift value: must be < 32, got {imm}");
        self.emit_push(imm, span);
        self.raw_exec("intrinsics::i32::checked_shr", span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and rotates the bits of `a` left by `b` bits
    ///
    /// Execution traps if `b` > 31.
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn rotl_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Rotl, span);
    }

    /// Pops a u32 value off the stack, `a`, and rotates the bits of `a` left by `imm` bits
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn rotl_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert!(imm < 32, "invalid rotation value: must be < 32, got {imm}");
        self.emit(masm::Instruction::U32RotlImm((imm as u8).into()), span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and rotates the bits of `a` right by `b`
    /// bits
    ///
    /// Execution traps if `b` > 31.
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn rotr_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Rotr, span);
    }

    /// Pops a u32 value off the stack, `a`, and rotates the bits of `a` right by `imm` bits
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn rotr_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        assert!(imm < 32, "invalid rotation value: must be < 32, got {imm}");
        self.emit(masm::Instruction::U32RotrImm((imm as u8).into()), span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and puts the result of `min(a, b)` on the
    /// stack
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn min_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Min, span);
    }

    /// Pops two i32 values off the stack, `b` and `a`, and puts the result of `min(a, b)` on the
    /// stack
    ///
    /// This operation is checked, if the operands or result are not valid i32, execution traps.
    pub fn min_i32(&mut self, span: SourceSpan) {
        self.raw_exec("intrinsics::i32::min", span);
    }

    /// Pops a u32 value off the stack, `a`, and puts the result of `min(a, imm)` on the stack
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn min_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        self.emit_push(imm, span);
        self.emit(masm::Instruction::U32Min, span);
    }

    /// Pops a i32 value off the stack, `a`, and puts the result of `min(a, imm)` on the stack
    ///
    /// This operation is checked, if the operand or result are not valid i32, execution traps.
    pub fn min_imm_i32(&mut self, imm: i32, span: SourceSpan) {
        self.emit_push(imm as u32, span);
        self.raw_exec("intrinsics::i32::min", span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and puts the result of `max(a, b)` on the
    /// stack
    ///
    /// This operation is checked, if the operands or result are not valid u32, execution traps.
    pub fn max_u32(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Max, span);
    }

    /// Pops two i32 values off the stack, `b` and `a`, and puts the result of `max(a, b)` on the
    /// stack
    ///
    /// This operation is checked, if the operands or result are not valid i32, execution traps.
    pub fn max_i32(&mut self, span: SourceSpan) {
        self.raw_exec("intrinsics::i32::max", span);
    }

    /// Pops a u32 value off the stack, `a`, and puts the result of `max(a, imm)` on the stack
    ///
    /// This operation is checked, if the operand or result are not valid u32, execution traps.
    pub fn max_imm_u32(&mut self, imm: u32, span: SourceSpan) {
        self.emit_push(imm, span);
        self.emit(masm::Instruction::U32Max, span);
    }

    /// Pops a i32 value off the stack, `a`, and puts the result of `max(a, imm)` on the stack
    ///
    /// This operation is checked, if the operand or result are not valid i32, execution traps.
    pub fn max_imm_i32(&mut self, imm: i32, span: SourceSpan) {
        self.emit_push(imm as u32, span);
        self.raw_exec("intrinsics::i32::max", span);
    }
}
