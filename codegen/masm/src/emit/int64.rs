use miden_core::Felt;
use midenc_hir::{Overflow, SourceSpan, Span};

use super::{OpEmitter, P, dup_from_offset, masm, movup_from_offset};

#[allow(unused)]
impl OpEmitter<'_> {
    /// Convert a u64 value to felt.
    ///
    /// This operation will assert at runtime if the value is larger than the felt field.
    pub fn u64_to_felt(&mut self, span: SourceSpan) {
        // Copy the input operand for the check
        self.copy_int64(span);
        // Assert that value is <= P, then unsplit the limbs to get a felt
        self.push_u64(P, span);
        self.lt_u64(span);
        self.emit(masm::Instruction::Assert, span);
        // `u32unsplit` expects `[hi, lo]` on the stack; u64 values are represented as `[lo, hi]`.
        self.emit(masm::Instruction::Swap1, span);
        self.u32unsplit(span);
    }

    /// Convert a i64 value to felt.
    ///
    /// This operation will assert at runtime if the value is negative, or larger than the felt
    /// field.
    #[inline]
    pub fn i64_to_felt(&mut self, span: SourceSpan) {
        self.u64_to_felt(span);
    }

    /// Convert a u64 value to an unsigned N-bit integer, where N <= 32
    ///
    /// Conversion will trap if the input value is too large to fit in an N-bit integer.
    pub fn u64_to_uint(&mut self, n: u32, span: SourceSpan) {
        // u64 values are stored on the operand stack in little-endian limb order, i.e. `[lo, hi]`
        // with `lo` on top. To validate truncation to <=32 bits, we must assert `hi == 0` and
        // then range-check `lo`.
        self.emit_all(
            [
                // Bring `hi` to the top of the stack and assert it is zero. This consumes `hi`,
                // leaving only `lo` on the stack.
                masm::Instruction::Swap1,
                masm::Instruction::Assertz,
                // Check that the remaining bits fit in range
                masm::Instruction::Dup0,
            ],
            span,
        );
        self.emit_push(Felt::new(2u64.pow(n) - 1), span);
        self.emit_all([masm::Instruction::U32Lte, masm::Instruction::Assert], span);
    }

    /// Convert an i64 value to a signed N-bit integer, where N <= 32
    ///
    /// Conversion will trap if the input value is too large to fit in an N-bit integer.
    pub fn i64_to_int(&mut self, n: u32, span: SourceSpan) {
        self.emit_all(
            [
                // Assert hi bits are all zero or all one
                // [x_hi, x_hi, x_lo]
                masm::Instruction::Dup0,
                // [is_unsigned, x_hi, x_lo]
                masm::Instruction::EqImm(Felt::ZERO.into()),
                // [is_unsigned, is_unsigned, ..]
                masm::Instruction::Dup0,
                // [is_unsigned, x_hi, is_unsigned, x_lo]
                masm::Instruction::MovDn2,
            ],
            span,
        );
        // Select all 0s if is_unsigned is true, else all 1s
        // [mask, x_hi, is_unsigned, x_lo]
        self.select_int32(0, u32::MAX, span);
        self.emit_all(
            [
                // [is_unsigned, x_lo]
                masm::Instruction::AssertEq,
                // [x_lo, is_unsigned, x_lo]
                masm::Instruction::Dup1,
            ],
            span,
        );
        // Select mask for remaining sign bits
        //
        // The mask should cover the u64 bits which must be set to 1 if
        // the value is in range for the N-bit integer type. If the value
        // is unsigned, the mask should be zero, so that comparing the
        // mask for equality succeeds in that case
        //
        // The value bits are all of the non-sign bits, so for an N-bit
        // integer, there are N-1 such bits.
        let value_bits = (2u64.pow(n - 1) - 1) as u32;
        // [sign_bits, is_unsigned, x_lo]
        self.const_mask_u32(!value_bits, span);
        // [sign_bits, sign_bits, ..]
        self.emit(masm::Instruction::Dup0, span);
        // [0, sign_bits, sign_bits, is_unsigned, x_lo]
        self.emit_push(0u32, span);
        self.emit_all(
            [
                // [is_unsigned, 0, sign_bits, sign_bits, x_lo]
                masm::Instruction::MovUp3,
                // [expected_sign_bits, sign_bits, x_lo]
                masm::Instruction::CDrop,
                // [x_lo]
                masm::Instruction::AssertEq,
            ],
            span,
        );
    }

    /// Truncate a i64/u64 value to a felt value
    ///
    /// This consumes the input value, and leaves a felt value on the stack.
    ///
    /// Truncation of field elements is not well-defined, as they have no specified
    /// binary representation. However, the u64 representation we use consists of two
    /// 32-bit limbs, and by multiplying the most significant limb by 2^32, and adding
    /// in the least significant limb, modulo `P` at each step, we obtain an equivalent
    /// felt to that we'd get from a typical bitwise truncation.
    ///
    /// Despite felt not having an exact bitwise representation (its range cannot be
    /// represented precisely using a power of two), truncating a u64 to felt, and felt to
    /// u32, is the same as truncating from u64 to u32.
    ///
    /// NOTE: This function does not validate the i64/u64, the caller is expected to
    /// have already validated that the top of the stack holds a valid value of this type.
    #[inline(always)]
    pub fn trunc_int64_to_felt(&mut self, span: SourceSpan) {
        // `u32unsplit` expects `[hi, lo]`; u64/i64 values are represented as `[lo, hi]`.
        self.emit(masm::Instruction::Swap1, span);
        self.u32unsplit(span)
    }

    /// Truncate this 64-bit value to N bits, where N is <= 32
    ///
    /// This consumes the input value, and leaves an N-bit value on the stack.
    ///
    /// NOTE: This function does not validate the i64/u64, the caller is expected to
    /// have already validated that the top of the stack holds a valid value of that type.
    #[inline]
    pub fn trunc_int64(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        // u64/i64 values are represented as `[lo, hi]` (lo on top). To truncate to <= 32 bits we
        // must drop the high limb and keep the low limb.
        self.emit_all([masm::Instruction::Swap1, masm::Instruction::Drop], span);
        match n {
            32 => (),
            n => self.trunc_int32(n, span),
        }
    }

    /// Sign-extend a 64-bit value to an signed N-bit integer, where N >= 128
    pub fn sext_int64(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 128, 256);
        let additional_limbs = (n / 32) - 2;

        // Move the most-significant limb to the top so the sign check inspects the actual i64
        // sign bit, rather than the low limb.
        self.emit(masm::Instruction::Swap1, span);
        self.is_signed_int32(span);
        self.select_int32(u32::MAX, 0, span);

        // `select_int32` leaves `[pad, hi, lo]` on the stack. Duplicate the sign-extension limb
        // as many times as needed, then rotate the original 64-bit value back to the top so the
        // final limb order remains little-endian: `[lo, hi, pad, pad, ...]`.
        self.emit_n(additional_limbs.saturating_sub(1) as usize, masm::Instruction::Dup0, span);
        self.emit(movup_from_offset((additional_limbs + 1) as usize), span);
        self.emit(movup_from_offset((additional_limbs + 1) as usize), span);
        self.emit(masm::Instruction::Swap1, span);
    }

    /// Zero-extend a 64-bit value to N-bits, where N >= 64
    pub fn zext_int64(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 128, 256);
        let additional_limbs = (n / 32) - 2;

        // Push the new most-significant limbs, then move the original 64-bit value back to the
        // top so the result stays in little-endian limb order: `[lo, hi, 0, 0, ...]`.
        self.emit_n(
            additional_limbs as usize,
            masm::Instruction::Push(masm::Immediate::Value(masm::Span::new(
                span,
                Felt::ZERO.into(),
            ))),
            span,
        );
        self.emit(movup_from_offset(additional_limbs as usize), span);
        self.emit(movup_from_offset((additional_limbs + 1) as usize), span);
        self.emit(masm::Instruction::Swap1, span);
    }

    /// Assert that there is a valid 64-bit integer value on the operand stack
    pub fn assert_int64(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Assert2, span);
    }

    /// Checks if the 64-bit value on the stack has its sign bit set.
    pub fn is_signed_int64(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::Swap1, span);
        self.is_signed_int32(span);
        self.emit_all([masm::Instruction::Swap1, masm::Instruction::MovDn2], span);
    }

    /// Assert that the 64-bit value on the stack does not have its sign bit set.
    pub fn assert_unsigned_int64(&mut self, span: SourceSpan) {
        // Assert that the sign bit is unset
        self.emit(masm::Instruction::Swap1, span);
        self.assert_unsigned_int32(span);
        self.emit(masm::Instruction::Swap1, span);
    }

    /// Assert that the 64-bit value on the stack is a valid i64 value
    pub fn assert_i64(&mut self, span: SourceSpan) {
        // Copy the value on top of the stack
        self.copy_int64(span);
        // Assert the value does not overflow i64::MAX or underflow i64::MIN
        // This can be checked by validating that when interpreted as a u64,
        // the value is <= i64::MIN, which is 1 more than i64::MAX.
        self.push_i64(i64::MIN, span);
        self.lte_u64(span);
        self.emit(masm::Instruction::Assert, span);
    }

    /// Duplicate the i64/u64 value on top of the stack
    #[inline(always)]
    pub fn copy_int64(&mut self, span: SourceSpan) {
        self.copy_int64_from(0, span)
    }

    /// Duplicate a i64/u64 value to the top of the stack
    ///
    /// The value `n` must be a valid stack index, and may not reference the last stack slot,
    /// or this function will panic.
    #[inline(always)]
    pub fn copy_int64_from(&mut self, n: u8, span: SourceSpan) {
        assert_valid_stack_index!(n + 1);
        // copy limbs such that the order is preserved
        self.emit_n(2, dup_from_offset(n as usize + 1), span);
    }

    /// Move a 64-bit value to the top of the stack, i.e. `movup(N)` for 64-bit values
    ///
    /// The value `n` must be a valid stack index, and may not reference the last stack slot,
    /// or this function will panic.
    ///
    /// A value of `0` has no effect.
    #[inline]
    pub fn move_int64_up(&mut self, n: u8, span: SourceSpan) {
        assert_valid_stack_index!(n + 1);
        match n {
            0 => (),
            1 => {
                // Move the top of the stack past the 64 bit value
                self.emit(masm::Instruction::MovDn2, span);
            }
            n => {
                let n = n as usize;
                self.emit_all(
                    [
                        // Move the low 32 bits to the top
                        movup_from_offset(n + 1),
                        // Move the high 32 bits to the top
                        movup_from_offset(n + 1),
                    ],
                    span,
                );
            }
        }
    }

    /// Pushes a literal i64 value on the operand stack
    #[inline(always)]
    pub fn push_i64(&mut self, value: i64, span: SourceSpan) {
        self.push_u64(value as u64, span);
    }

    /// Pushes a literal u64 value on the operand stack
    #[inline]
    pub fn push_u64(&mut self, value: u64, span: SourceSpan) {
        let (hi, lo) = to_raw_parts(value);
        from_raw_parts(lo, hi, self.current_block, span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a < b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn lt_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::lt", span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `a < b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn lt_i64(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::i64::lt", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a <= b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn lte_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::lte", span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `a <= b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn lte_i64(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::i64::lte", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a > b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn gt_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::gt", span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `a > b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn gt_i64(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::i64::gt", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a >= b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn gte_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::gte", span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `a >= b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn gte_i64(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::i64::gte", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a == b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn eq_int64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::eq", span);
    }

    /// Pops a u64 value off the stack, `a`, and pushes `a == 0` on the stack.
    ///
    /// This operation is checked, so if the value is not a valid u64, execution will trap.
    #[inline]
    pub fn is_zero_int64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::eqz", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `min(a, b)` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn min_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::min", span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `min(a, b)` on the stack.
    ///
    /// This operation is checked, so if the values are not valid i64, execution will trap.
    pub fn min_i64(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::i64::min", span);
    }

    pub fn min_imm_i64(&mut self, imm: i64, span: SourceSpan) {
        self.push_i64(imm, span);
        self.raw_exec("::intrinsics::i64::min", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `max(a, b)` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn max_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::max", span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `max(a, b)` on the stack.
    ///
    /// This operation is checked, so if the values are not valid i64, execution will trap.
    pub fn max_i64(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::i64::max", span);
    }

    pub fn max_imm_i64(&mut self, imm: i64, span: SourceSpan) {
        self.push_i64(imm, span);
        self.raw_exec("::intrinsics::i64::max", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a != b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn neq_int64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::neq", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and performs `a + b`.
    ///
    /// The semantics of this operation depend on the `overflow` setting:
    ///
    /// * There is no unchecked variant for u64, so wrapping is used instead
    /// * When checked, both the operands and the result are validated to ensure they are valid u64
    ///   values.
    /// * Overflowing and wrapping variants follow the usual semantics, with the exception that
    ///   neither type validates the operands, it is assumed that the caller has already done this.
    ///
    /// The caller is assumed to know that different `overflow` settings can
    /// produce different results, and that those differences are handled.
    #[inline]
    pub fn add_u64(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Checked => {
                self.raw_exec("::miden::core::math::u64::overflowing_add", span);
                self.emit(masm::Instruction::Assertz, span);
            }
            Overflow::Unchecked | Overflow::Wrapping => {
                self.raw_exec("::miden::core::math::u64::wrapping_add", span);
            }
            Overflow::Overflowing => {
                self.raw_exec("::miden::core::math::u64::overflowing_add", span);
            }
        }
    }

    /// Pops two i64 values off the stack, `b` and `a`, and performs `a + b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    #[inline(always)]
    pub fn add_i64(&mut self, overflow: Overflow, span: SourceSpan) {
        self.raw_exec(
            match overflow {
                Overflow::Unchecked | Overflow::Wrapping => {
                    "::miden::core::math::u64::wrapping_add"
                }
                Overflow::Checked => "::intrinsics::i64::checked_add",
                Overflow::Overflowing => "::intrinsics::i64::overflowing_add",
            },
            span,
        )
    }

    /// Pops a i64 value off the stack, `a`, and performs `a + <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Adding zero is a no-op.
    #[inline]
    pub fn add_imm_i64(&mut self, imm: i64, overflow: Overflow, span: SourceSpan) {
        if imm == 0 {
            return;
        }
        self.push_i64(imm, span);
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => self.add_u64(overflow, span),
            Overflow::Checked => {
                self.raw_exec("::intrinsics::i64::checked_add", span);
            }
            Overflow::Overflowing => self.raw_exec("::intrinsics::i64::overflowing_add", span),
        }
    }

    /// Pops two u64 values off the stack, `b` and `a`, and performs `a - b`.
    ///
    /// The semantics of this operation depend on the `overflow` setting:
    ///
    /// * There is no unchecked variant for u64, so wrapping is used instead
    /// * When checked, both the operands and the result are validated to ensure they are valid u64
    ///   values.
    /// * Overflowing and wrapping variants follow the usual semantics, with the exception that
    ///   neither type validates the operands, it is assumed that the caller has already done this.
    ///
    /// The caller is assumed to know that different `overflow` settings can
    /// produce different results, and that those differences are handled.
    #[inline]
    pub fn sub_u64(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Checked => {
                self.raw_exec("::miden::core::math::u64::overflowing_sub", span);
                self.emit(masm::Instruction::Assertz, span);
            }
            Overflow::Unchecked | Overflow::Wrapping => {
                self.raw_exec("::miden::core::math::u64::wrapping_sub", span);
            }
            Overflow::Overflowing => {
                self.raw_exec("::miden::core::math::u64::overflowing_sub", span);
            }
        }
    }

    /// Pops two i64 values off the stack, `b` and `a`, and performs `a - b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    pub fn sub_i64(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => self.sub_u64(overflow, span),
            Overflow::Checked => self.raw_exec("::intrinsics::i64::checked_sub", span),
            Overflow::Overflowing => self.raw_exec("::intrinsics::i64::overflowing_sub", span),
        }
    }

    /// Pops a i64 value off the stack, `a`, and performs `a - <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Subtracting zero is a no-op.
    #[inline]
    pub fn sub_imm_i64(&mut self, imm: i64, overflow: Overflow, span: SourceSpan) {
        if imm == 0 {
            return;
        }
        self.push_i64(imm, span);
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => self.sub_u64(overflow, span),
            Overflow::Checked => self.raw_exec("::intrinsics::i64::checked_sub", span),
            Overflow::Overflowing => self.raw_exec("::intrinsics::i64::overflowing_sub", span),
        }
    }

    /// Pops two u64 values off the stack, `b` and `a`, and performs `a * b`.
    ///
    /// The semantics of this operation depend on the `overflow` setting:
    ///
    /// * There is no unchecked variant for u64, so wrapping is used instead
    /// * When checked, both the operands and the result are validated to ensure they are valid u64
    ///   values.
    /// * Overflowing and wrapping variants follow the usual semantics, with the exception that
    ///   neither type validates the operands, it is assumed that the caller has already done this.
    ///
    /// The caller is assumed to know that different `overflow` settings can
    /// produce different results, and that those differences are handled.
    #[inline]
    pub fn mul_u64(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Checked => {
                self.raw_exec("::miden::core::math::u64::widening_mul", span);
                // `widening_mul` returns a u128: `[c0, c1, c2, c3]` where `c0` is the low limb.
                //
                // To check that the result fits in u64, we must verify that the upper 64-bits
                // (i.e. `c2` and `c3`) are zero. We compute an overflow flag and then assert it is
                // zero, leaving only the low 64-bits (`c0`, `c1`) on the stack.
                self.emit_all(
                    [
                        // overflow = (c2 != 0) || (c3 != 0)
                        masm::Instruction::Dup2,
                        masm::Instruction::EqImm(Felt::ZERO.into()),
                        masm::Instruction::Not,
                        masm::Instruction::Dup4,
                        masm::Instruction::EqImm(Felt::ZERO.into()),
                        masm::Instruction::Not,
                        masm::Instruction::Or,
                        // Move overflow to the bottom so we can drop the upper limbs
                        masm::Instruction::MovDn4,
                        // Drop c3
                        masm::Instruction::MovUp3,
                        masm::Instruction::Drop,
                        // Drop c2
                        masm::Instruction::MovUp2,
                        masm::Instruction::Drop,
                        // Bring overflow back to the top and assert it is zero
                        masm::Instruction::MovUp2,
                        masm::Instruction::Assertz,
                    ],
                    span,
                );
            }
            Overflow::Unchecked | Overflow::Wrapping => {
                self.raw_exec("::miden::core::math::u64::wrapping_mul", span);
            }
            Overflow::Overflowing => {
                self.raw_exec("::miden::core::math::u64::widening_mul", span);
                // Return `[overflow, c_lo, c_hi]`, where `overflow` is 1 iff the upper 64 bits are
                // non-zero.
                self.emit_all(
                    [
                        // overflow = (c2 != 0) || (c3 != 0)
                        masm::Instruction::Dup2,
                        masm::Instruction::EqImm(Felt::ZERO.into()),
                        masm::Instruction::Not,
                        masm::Instruction::Dup4,
                        masm::Instruction::EqImm(Felt::ZERO.into()),
                        masm::Instruction::Not,
                        masm::Instruction::Or,
                        // Move overflow to the bottom so we can drop the upper limbs
                        masm::Instruction::MovDn4,
                        // Drop c3
                        masm::Instruction::MovUp3,
                        masm::Instruction::Drop,
                        // Drop c2
                        masm::Instruction::MovUp2,
                        masm::Instruction::Drop,
                        // Bring overflow back to the top
                        masm::Instruction::MovUp2,
                    ],
                    span,
                );
            }
        }
    }

    /// Pops two i64 values off the stack, `b` and `a`, and performs `a * b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    pub fn mul_i64(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => {
                self.raw_exec("::intrinsics::i64::wrapping_mul", span)
            }
            Overflow::Checked => self.raw_exec("::intrinsics::i64::checked_mul", span),
            Overflow::Overflowing => self.raw_exec("::intrinsics::i64::overflowing_mul", span),
        }
    }

    /// Pops a i64 value off the stack, `a`, and performs `a * <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Multiplying by zero is transformed into a sequence which drops the input value
    /// and pushes a constant zero on the stack.
    ///
    /// Multiplying by one is a no-op.
    #[inline]
    pub fn mul_imm_i64(&mut self, imm: i64, overflow: Overflow, span: SourceSpan) {
        match imm {
            0 => {
                self.emit_all([masm::Instruction::Drop, masm::Instruction::Drop], span);
                self.emit_push(0u32, span);
                self.emit_push(0u32, span);
            }
            1 => (),
            imm => match overflow {
                Overflow::Unchecked | Overflow::Wrapping => {
                    self.push_i64(imm, span);
                    self.raw_exec("::intrinsics::i64::wrapping_mul", span);
                }
                Overflow::Checked => {
                    self.push_i64(imm, span);
                    self.raw_exec("::intrinsics::i64::checked_mul", span);
                }
                Overflow::Overflowing => {
                    self.push_i64(imm, span);
                    self.raw_exec("::intrinsics::i64::overflowing_mul", span);
                }
            },
        }
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes the result of `a / b` on the
    /// stack.
    ///
    /// Both the operands and result are validated to ensure they are valid u64 values.
    #[inline]
    pub fn checked_div_u64(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32AssertW, span);
        self.raw_exec("::miden::core::math::u64::div", span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes the result of `a / b` on the
    /// stack.
    ///
    /// Both the operands and result are validated to ensure they are valid u64 values.
    #[inline]
    pub fn checked_div_i64(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::i64::checked_div", span);
    }

    /// Pops a i64 value off the stack, `a`, and performs `a / <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    ///
    /// This operation is checked, so if the operand or result are not valid i32, execution traps.
    pub fn checked_div_imm_i64(&mut self, imm: i64, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.push_i64(imm, span);
        self.raw_exec("::intrinsics::i64::checked_div", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes the result of `a / b` on the
    /// stack.
    ///
    /// This operation is unchecked, it is up to the caller to ensure validity of the operands.
    #[inline]
    pub fn unchecked_div_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::div", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes the result of `a % b` on the
    /// stack.
    ///
    /// Both the operands and result are validated to ensure they are valid u64 values.
    #[inline]
    pub fn checked_mod_u64(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32AssertW, span);
        self.raw_exec("::miden::core::math::u64::mod", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes the result of `a % b` on the
    /// stack.
    ///
    /// This operation is unchecked, it is up to the caller to ensure validity of the operands.
    #[inline]
    pub fn unchecked_mod_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::mod", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a / b`, then `a % b` on the
    /// stack.
    ///
    /// Both the operands and result are validated to ensure they are valid u64 values.
    #[inline]
    pub fn checked_divmod_u64(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32AssertW, span);
        self.raw_exec("::miden::core::math::u64::divmod", span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a / b`, then `a % b` on the
    /// stack.
    ///
    /// This operation is unchecked, it is up to the caller to ensure validity of the operands.
    #[inline]
    pub fn unchecked_divmod_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::divmod", span);
    }

    /// Pops two 64-bit values off the stack, `b` and `a`, and pushes `a & b` on the stack.
    ///
    /// Both the operands and result are validated to ensure they are valid int64 values.
    #[inline]
    pub fn band_int64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::and", span);
    }

    /// Pops two 64-bit values off the stack, `b` and `a`, and pushes `a | b` on the stack.
    ///
    /// Both the operands and result are validated to ensure they are valid int64 values.
    #[inline]
    pub fn bor_int64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::or", span);
    }

    /// Pops two 64-bit values off the stack, `b` and `a`, and pushes `a ^ b` on the stack.
    ///
    /// Both the operands and result are validated to ensure they are valid int64 values.
    #[inline]
    pub fn bxor_int64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::xor", span);
    }

    /// Pops a u32 value, `b`, and a u64 value, `a`, off the stack and pushes `a << b` on the stack.
    ///
    /// Overflow bits are truncated.
    ///
    /// The operation will trap if the shift value is > 63.
    #[inline]
    pub fn shl_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::shl", span);
    }

    /// Pops a u32 value, `b`, and a u64 value, `a`, off the stack and pushes `a >> b` on the stack.
    ///
    /// Overflow bits are truncated.
    ///
    /// The operation will trap if the shift value is > 63.
    #[inline]
    pub fn shr_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::shr", span);
    }

    /// Arithmetic shift right (i.e. signedness is preserved)
    ///
    /// Pops a u32 value, `b`, and a i64 value, `a`, off the stack and pushes `a >> b` on the stack.
    ///
    /// Overflow bits are truncated.
    ///
    /// The operation will trap if the shift value is > 63.
    #[inline]
    pub fn shr_i64(&mut self, span: SourceSpan) {
        self.raw_exec("::intrinsics::i64::checked_shr", span);
    }

    /// Pops a i64 value off the stack, `a`, and performs `a >> <imm>`
    ///
    /// This operation is checked, if the operand or result are not valid i64, execution traps.
    pub fn shr_imm_i64(&mut self, imm: u32, span: SourceSpan) {
        assert!(imm < 63, "invalid shift value: must be < 63, got {imm}");
        self.emit_push(imm, span);
        self.raw_exec("::intrinsics::i64::checked_shr", span);
    }

    /// Pops a u32 value, `b`, and a u64 value, `a`, off the stack and rotates the bitwise
    /// representation of `a` left `b` bits. Any values that are rotated past the most significant
    /// bit, wrap around to the least significant bit.
    ///
    /// The operation will trap if the rotation value is > 63.
    #[inline]
    pub fn rotl_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::rotl", span);
    }

    /// Pops a u32 value, `b`, and a u64 value, `a`, off the stack and rotates the bitwise
    /// representation of `a` right `b` bits. Any values that are rotated past the least significant
    /// bit, wrap around to the most significant bit.
    ///
    /// The operation will trap if the rotation value is > 63.
    #[inline]
    pub fn rotr_u64(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u64::rotr", span);
    }
}

/// Decompose a u64 value into it's raw 32-bit limb components
///
/// Returns `(hi, lo)`, where `hi` is the most significant limb,
/// and `lo` is the least significant limb.
#[inline(always)]
pub fn to_raw_parts(value: u64) -> (u32, u32) {
    let bytes = value.to_be_bytes();
    let hi = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let lo = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    (hi, lo)
}

/// Construct a u64/i64 constant from raw parts, i.e. two 32-bit little-endian limbs
///
/// Pushes hi first, then lo, so the stack ends up as [lo, hi] (lo on top) matching the LE
/// convention where the low limb is on top.
#[inline]
pub fn from_raw_parts(lo: u32, hi: u32, block: &mut Vec<masm::Op>, span: SourceSpan) {
    block.push(masm::Op::Inst(Span::new(
        span,
        masm::Instruction::Push(masm::Immediate::Value(Span::new(
            span,
            Felt::new(hi as u64).into(),
        ))),
    )));
    block.push(masm::Op::Inst(Span::new(
        span,
        masm::Instruction::Push(masm::Immediate::Value(Span::new(
            span,
            Felt::new(lo as u64).into(),
        ))),
    )));
}
