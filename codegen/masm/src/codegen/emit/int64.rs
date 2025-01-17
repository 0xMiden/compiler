use midenc_hir::{Felt, FieldElement, Overflow, SourceSpan};

use super::{OpEmitter, P};
use crate::masm::{self as masm, Op};

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
        self.emit(Op::Assert, span);
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
        self.emit_all(
            &[
                // Assert hi bits are zero
                Op::Assertz,
                // Check that the remaining bits fit in range
                Op::Dup(0),
                Op::Push(Felt::new(2u64.pow(n) - 1)),
                Op::U32Lte,
                Op::Assert,
            ],
            span,
        );
    }

    /// Convert an i64 value to a signed N-bit integer, where N <= 32
    ///
    /// Conversion will trap if the input value is too large to fit in an N-bit integer.
    pub fn i64_to_int(&mut self, n: u32, span: SourceSpan) {
        self.emit_all(
            &[
                // Assert hi bits are all zero or all one
                // [x_hi, x_hi, x_lo]
                Op::Dup(0),
                // [is_unsigned, x_hi, x_lo]
                Op::EqImm(Felt::ZERO),
                // [is_unsigned, is_unsigned, ..]
                Op::Dup(0),
                // [is_unsigned, x_hi, is_unsigned, x_lo]
                Op::Movdn(2),
            ],
            span,
        );
        // Select all 0s if is_unsigned is true, else all 1s
        // [mask, x_hi, is_unsigned, x_lo]
        self.select_int32(0, u32::MAX, span);
        self.emit_all(
            &[
                // [is_unsigned, x_lo]
                Op::AssertEq,
                // [x_lo, is_unsigned, x_lo]
                Op::Dup(1),
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
        self.emit_all(
            &[
                // [sign_bits, sign_bits, ..]
                Op::Dup(0),
                // [0, sign_bits, sign_bits, is_unsigned, x_lo]
                Op::PushU32(0),
                // [is_unsigned, 0, sign_bits, sign_bits, x_lo]
                Op::Movup(3),
                // [expected_sign_bits, sign_bits, x_lo]
                Op::Cdrop,
                // [x_lo]
                Op::AssertEq,
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
        self.emit(Op::Drop, span);
        match n {
            32 => (),
            n => self.trunc_int32(n, span),
        }
    }

    /// Sign-extend a 64-bit value to an signed N-bit integer, where N >= 128
    pub fn sext_int64(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 128, 256);
        self.is_signed_int64(span);
        // Select the extension bits
        self.select_int32(u32::MAX, 0, span);
        // Pad out the missing bits
        //
        // Deduct 32 bits to account for the difference between u32 and u64
        self.pad_int32(n - 32, span);
    }

    /// Zero-extend a 64-bit value to N-bits, where N >= 64
    pub fn zext_int64(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 128, 256);
        // Pad out the missing bits
        //
        // Deduct 32 bits to account for the difference between u32 and u64
        self.zext_int32(n - 32, span);
    }

    /// Assert that there is a valid 64-bit integer value on the operand stack
    pub fn assert_int64(&mut self, span: SourceSpan) {
        self.emit(Op::U32Assert2, span);
    }

    /// Checks if the 64-bit value on the stack has its sign bit set.
    #[inline(always)]
    pub fn is_signed_int64(&mut self, span: SourceSpan) {
        self.is_signed_int32(span)
    }

    /// Assert that the 64-bit value on the stack does not have its sign bit set.
    #[inline(always)]
    pub fn assert_unsigned_int64(&mut self, span: SourceSpan) {
        // Assert that the sign bit is unset
        self.assert_unsigned_int32(span)
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
        self.emit(Op::Assert, span);
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
        self.emit_n(2, Op::Dup(n + 1), span);
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
                self.emit(Op::Movdn(2), span);
            }
            n => {
                self.emit_all(
                    &[
                        // Move the low 32 bits to the top
                        Op::Movup(n + 1),
                        // Move the high 32 bits to the top
                        Op::Movup(n + 1),
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
        from_raw_parts(lo, hi, self.current_block(), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a < b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn lt_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::lt".parse().unwrap()), span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `a < b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn lt_i64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::i64::lt".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a <= b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn lte_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::lte".parse().unwrap()), span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `a <= b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn lte_i64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::i64::lte".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a > b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn gt_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::gt".parse().unwrap()), span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `a > b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn gt_i64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::i64::gt".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a >= b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn gte_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::gte".parse().unwrap()), span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `a >= b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn gte_i64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::i64::gte".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a == b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn eq_int64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::eq".parse().unwrap()), span);
    }

    /// Pops a u64 value off the stack, `a`, and pushes `a == 0` on the stack.
    ///
    /// This operation is checked, so if the value is not a valid u64, execution will trap.
    #[inline]
    pub fn is_zero_int64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::eqz".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `min(a, b)` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn min_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::min".parse().unwrap()), span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `min(a, b)` on the stack.
    ///
    /// This operation is checked, so if the values are not valid i64, execution will trap.
    pub fn min_i64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::i64::min".parse().unwrap()), span);
    }

    pub fn min_imm_i64(&mut self, imm: i64, span: SourceSpan) {
        self.push_i64(imm, span);
        self.emit(Op::Exec("intrinsics::i64::min".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `max(a, b)` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn max_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::max".parse().unwrap()), span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes `max(a, b)` on the stack.
    ///
    /// This operation is checked, so if the values are not valid i64, execution will trap.
    pub fn max_i64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::i64::max".parse().unwrap()), span);
    }

    pub fn max_imm_i64(&mut self, imm: i64, span: SourceSpan) {
        self.push_i64(imm, span);
        self.emit(Op::Exec("intrinsics::i64::max".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a != b` on the stack.
    ///
    /// This operation is checked, so if the values are not valid u64, execution will trap.
    #[inline]
    pub fn neq_int64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::neq".parse().unwrap()), span);
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
                self.emit_all(
                    &[Op::Exec("std::math::u64::overflowing_add".parse().unwrap()), Op::Assertz],
                    span,
                );
            }
            Overflow::Unchecked | Overflow::Wrapping => {
                self.emit(Op::Exec("std::math::u64::wrapping_add".parse().unwrap()), span);
            }
            Overflow::Overflowing => {
                self.emit(Op::Exec("std::math::u64::overflowing_add".parse().unwrap()), span);
            }
        }
    }

    /// Pops two i64 values off the stack, `b` and `a`, and performs `a + b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    #[inline(always)]
    pub fn add_i64(&mut self, overflow: Overflow, span: SourceSpan) {
        self.emit(
            match overflow {
                Overflow::Unchecked | Overflow::Wrapping => {
                    Op::Exec("std::math::u64::wrapping_add".parse().unwrap())
                }
                Overflow::Checked => Op::Exec("intrinsics::i64::checked_add".parse().unwrap()),
                Overflow::Overflowing => {
                    Op::Exec("intrinsics::i64::overflowing_add".parse().unwrap())
                }
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
                self.emit(Op::Exec("intrinsics::i64::checked_add".parse().unwrap()), span);
            }
            Overflow::Overflowing => {
                self.emit(Op::Exec("intrinsics::i64::overflowing_add".parse().unwrap()), span)
            }
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
                self.emit_all(
                    &[Op::Exec("std::math::u64::overflowing_sub".parse().unwrap()), Op::Assertz],
                    span,
                );
            }
            Overflow::Unchecked | Overflow::Wrapping => {
                self.emit(Op::Exec("std::math::u64::wrapping_sub".parse().unwrap()), span);
            }
            Overflow::Overflowing => {
                self.emit(Op::Exec("std::math::u64::overflowing_sub".parse().unwrap()), span);
            }
        }
    }

    /// Pops two i64 values off the stack, `b` and `a`, and performs `a - b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    pub fn sub_i64(&mut self, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked | Overflow::Wrapping => self.sub_u64(overflow, span),
            Overflow::Checked => {
                self.emit(Op::Exec("intrinsics::i64::checked_sub".parse().unwrap()), span)
            }
            Overflow::Overflowing => {
                self.emit(Op::Exec("intrinsics::i64::overflowing_sub".parse().unwrap()), span)
            }
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
            Overflow::Checked => {
                self.emit(Op::Exec("intrinsics::i64::checked_sub".parse().unwrap()), span)
            }
            Overflow::Overflowing => {
                self.emit(Op::Exec("intrinsics::i64::overflowing_sub".parse().unwrap()), span)
            }
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
                self.emit_all(
                    &[
                        Op::Exec("std::math::u64::overflowing_mul".parse().unwrap()),
                        Op::Exec("std::math::u64::overflowing_eqz".parse().unwrap()),
                        Op::Assertz,
                    ],
                    span,
                );
            }
            Overflow::Unchecked | Overflow::Wrapping => {
                self.emit(Op::Exec("std::math::u64::wrapping_mul".parse().unwrap()), span);
            }
            Overflow::Overflowing => {
                self.emit_all(
                    &[
                        Op::Exec("std::math::u64::overflowing_mul".parse().unwrap()),
                        Op::Exec("std::math::u64::overflowing_eqz".parse().unwrap()),
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
                self.emit(Op::Exec("intrinsics::i64::wrapping_mul".parse().unwrap()), span)
            }
            Overflow::Checked => {
                self.emit(Op::Exec("intrinsics::i64::checked_mul".parse().unwrap()), span)
            }
            Overflow::Overflowing => {
                self.emit(Op::Exec("intrinsics::i64::overflowing_mul".parse().unwrap()), span)
            }
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
                self.emit_all(&[Op::Drop, Op::Drop, Op::PushU32(0), Op::PushU32(0)], span);
            }
            1 => (),
            imm => match overflow {
                Overflow::Unchecked | Overflow::Wrapping => {
                    self.push_i64(imm, span);
                    self.emit(Op::Exec("intrinsics::i64::wrapping_mul".parse().unwrap()), span);
                }
                Overflow::Checked => {
                    self.push_i64(imm, span);
                    self.emit(Op::Exec("intrinsics::i64::checked_mul".parse().unwrap()), span);
                }
                Overflow::Overflowing => {
                    self.push_i64(imm, span);
                    self.emit(Op::Exec("intrinsics::i64::overflowing_mul".parse().unwrap()), span);
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
        self.emit_all(&[Op::U32Assertw, Op::Exec("std::math::u64::div".parse().unwrap())], span);
    }

    /// Pops two i64 values off the stack, `b` and `a`, and pushes the result of `a / b` on the
    /// stack.
    ///
    /// Both the operands and result are validated to ensure they are valid u64 values.
    #[inline]
    pub fn checked_div_i64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("intrinsics::i64::checked_div".parse().unwrap()), span);
    }

    /// Pops a i64 value off the stack, `a`, and performs `a / <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    ///
    /// This operation is checked, so if the operand or result are not valid i32, execution traps.
    pub fn checked_div_imm_i64(&mut self, imm: i64, span: SourceSpan) {
        assert_ne!(imm, 0, "division by zero is not allowed");
        self.push_i64(imm, span);
        self.emit(Op::Exec("intrinsics::i64::checked_div".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes the result of `a / b` on the
    /// stack.
    ///
    /// This operation is unchecked, it is up to the caller to ensure validity of the operands.
    #[inline]
    pub fn unchecked_div_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::div".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes the result of `a % b` on the
    /// stack.
    ///
    /// Both the operands and result are validated to ensure they are valid u64 values.
    #[inline]
    pub fn checked_mod_u64(&mut self, span: SourceSpan) {
        self.emit_all(&[Op::U32Assertw, Op::Exec("std::math::u64::mod".parse().unwrap())], span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes the result of `a % b` on the
    /// stack.
    ///
    /// This operation is unchecked, it is up to the caller to ensure validity of the operands.
    #[inline]
    pub fn unchecked_mod_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::mod".parse().unwrap()), span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a / b`, then `a % b` on the
    /// stack.
    ///
    /// Both the operands and result are validated to ensure they are valid u64 values.
    #[inline]
    pub fn checked_divmod_u64(&mut self, span: SourceSpan) {
        self.emit_all(&[Op::U32Assertw, Op::Exec("std::math::u64::divmod".parse().unwrap())], span);
    }

    /// Pops two u64 values off the stack, `b` and `a`, and pushes `a / b`, then `a % b` on the
    /// stack.
    ///
    /// This operation is unchecked, it is up to the caller to ensure validity of the operands.
    #[inline]
    pub fn unchecked_divmod_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::divmod".parse().unwrap()), span);
    }

    /// Pops two 64-bit values off the stack, `b` and `a`, and pushes `a & b` on the stack.
    ///
    /// Both the operands and result are validated to ensure they are valid int64 values.
    #[inline]
    pub fn band_int64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::and".parse().unwrap()), span);
    }

    /// Pops two 64-bit values off the stack, `b` and `a`, and pushes `a | b` on the stack.
    ///
    /// Both the operands and result are validated to ensure they are valid int64 values.
    #[inline]
    pub fn bor_int64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::or".parse().unwrap()), span);
    }

    /// Pops two 64-bit values off the stack, `b` and `a`, and pushes `a ^ b` on the stack.
    ///
    /// Both the operands and result are validated to ensure they are valid int64 values.
    #[inline]
    pub fn bxor_int64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::xor".parse().unwrap()), span);
    }

    /// Pops a u32 value, `b`, and a u64 value, `a`, off the stack and pushes `a << b` on the stack.
    ///
    /// Overflow bits are truncated.
    ///
    /// The operation will trap if the shift value is > 63.
    #[inline]
    pub fn shl_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::shl".parse().unwrap()), span);
    }

    /// Pops a u32 value, `b`, and a u64 value, `a`, off the stack and pushes `a >> b` on the stack.
    ///
    /// Overflow bits are truncated.
    ///
    /// The operation will trap if the shift value is > 63.
    #[inline]
    pub fn shr_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::shr".parse().unwrap()), span);
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
        self.emit(Op::Exec("intrinsics::i64::checked_shr".parse().unwrap()), span);
    }

    /// Pops a i64 value off the stack, `a`, and performs `a >> <imm>`
    ///
    /// This operation is checked, if the operand or result are not valid i64, execution traps.
    pub fn shr_imm_i64(&mut self, imm: u32, span: SourceSpan) {
        assert!(imm < 63, "invalid shift value: must be < 63, got {imm}");
        self.emit_all(
            &[Op::PushU32(imm), Op::Exec("intrinsics::i64::checked_shr".parse().unwrap())],
            span,
        );
    }

    /// Pops a u32 value, `b`, and a u64 value, `a`, off the stack and rotates the bitwise
    /// representation of `a` left `b` bits. Any values that are rotated past the most significant
    /// bit, wrap around to the least significant bit.
    ///
    /// The operation will trap if the rotation value is > 63.
    #[inline]
    pub fn rotl_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::rotl".parse().unwrap()), span);
    }

    /// Pops a u32 value, `b`, and a u64 value, `a`, off the stack and rotates the bitwise
    /// representation of `a` right `b` bits. Any values that are rotated past the least significant
    /// bit, wrap around to the most significant bit.
    ///
    /// The operation will trap if the rotation value is > 63.
    #[inline]
    pub fn rotr_u64(&mut self, span: SourceSpan) {
        self.emit(Op::Exec("std::math::u64::rotr".parse().unwrap()), span);
    }
}

/// Decompose a u64 value into it's raw 32-bit limb components
///
/// Returns `(hi, lo)`, where `hi` is the most significant limb,
/// and `lo` is the least significant limb.
#[inline(always)]
pub fn to_raw_parts(value: u64) -> (u32, u32) {
    let bytes = value.to_le_bytes();
    let hi = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let lo = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    (hi, lo)
}

/// Construct a u64/i64 constant from raw parts, i.e. two 32-bit little-endian limbs
#[inline]
pub fn from_raw_parts(lo: u32, hi: u32, block: &mut masm::Block, span: SourceSpan) {
    block.push(Op::Push2([Felt::new(lo as u64), Felt::new(hi as u64)]), span);
}
