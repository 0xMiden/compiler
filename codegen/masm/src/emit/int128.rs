//! A 128-bit integer is four 32-bit limbs on the operand stack, in little-endian order: the least
//! significant limb is on top, `[x0, x1, x2, x3]`, for the value `x0 + x1 * 2^32 + x2 * 2^64 +
//! x3 * 2^96`. This is the layout of `u128` in the core library (`::miden::core::math::u128`).
//!
//! Each half is laid out as a 64-bit integer is, `[lo, hi]`: the low half `[x0, x1]` is on top of
//! the high half `[x2, x3]`.

use miden_core::Felt;
use midenc_hir::{Overflow, SourceSpan};

use super::{OpEmitter, int32::SIGN_BIT, masm};

#[allow(unused)]
impl OpEmitter<'_> {
    /// Checks if the i128 value on the stack has its sign bit set, the most significant bit of
    /// `x3`.
    ///
    /// The value IS NOT consumed: `[x0, x1, x2, x3] => [is_signed, x0, x1, x2, x3]`
    #[inline(always)]
    pub fn is_signed_int128(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::Dup3, span);
        self.const_mask_u32(SIGN_BIT, span);
        self.emit(masm::Instruction::EqImm(Felt::new_unchecked(SIGN_BIT as u64).into()), span);
    }

    /// Assert that the i128 value on the stack does not have its sign bit set.
    ///
    /// The value IS NOT consumed.
    #[inline(always)]
    pub fn assert_unsigned_int128(&mut self, span: SourceSpan) {
        self.is_signed_int128(span);
        self.emit(
            Self::assertz_with_message_inst("expected a non-negative i128 value", span),
            span,
        );
    }

    /// Push a u128 value on the operand stack
    ///
    /// An u128 value consists of 4 32-bit limbs; the high half is pushed first, so that the least
    /// significant limb ends up on top.
    pub fn push_u128(&mut self, value: u128, span: SourceSpan) {
        let lo = value as u64;
        let hi = (value >> 64) as u64;
        self.push_u64(hi, span);
        self.push_u64(lo, span);
    }

    /// Push an i128 value on the operand stack
    ///
    /// An i128 value consists of 4 32-bit limbs
    #[inline(always)]
    pub fn push_i128(&mut self, value: i128, span: SourceSpan) {
        self.push_u128(value as u128, span);
    }

    /// Convert an i128 value to a field element value.
    ///
    /// This is different than `trunc_i128_to_felt`, as this function performs a
    /// range check on the input value to ensure that it will fit in a felt.
    ///
    /// This consumes the input value, and leaves a felt value on the stack.
    /// Execution traps if the input value cannot fit in a field element.
    ///
    /// NOTE: This function does not validate the i128, the caller is expected to
    /// have already validated that the top of the stack holds a valid i128.
    pub fn int128_to_felt(&mut self, span: SourceSpan) {
        // First, convert to u64
        self.int128_to_u64(span);
        // Then convert the u64 to felt
        self.u64_to_felt(span);
    }

    /// Convert a 128-bit value to u64
    ///
    /// This is different than `trunc_i128`, as this function performs a
    /// range check on the input value to ensure that it will fit in a u64.
    ///
    /// This consumes the input value, and leaves a u64 value on the stack.
    ///
    /// NOTE: This function does not validate the i128, the caller is expected to
    /// have already validated that the top of the stack holds a valid i128.
    pub fn int128_to_u64(&mut self, span: SourceSpan) {
        // Assert the two most significant limbs are equal to 0
        //
        // What remains on the stack at this point are the low 64-bits,
        // which is also our result: `[x0, x1, x2, x3] => [x0, x1]`
        let assertz = Self::assertz_with_message_inst("128-bit value does not fit in u64", span);
        self.emit_all(
            [masm::Instruction::MovUp3, assertz.clone(), masm::Instruction::MovUp2, assertz],
            span,
        );
    }

    /// Convert a 128-bit value to u32
    ///
    /// This is different than `trunc_i128`, as this function performs a
    /// range check on the input value to ensure that it will fit in a u32.
    ///
    /// This consumes the input value, and leaves a u32 value on the stack.
    ///
    /// NOTE: This function does not validate the i128, the caller is expected to
    /// have already validated that the top of the stack holds a valid i128.
    pub fn int128_to_u32(&mut self, span: SourceSpan) {
        // Move the least significant limb below the three others, and assert they are equal to 0
        //
        // What remains on the stack at this point are the low 32-bits,
        // which is also our result: `[x0, x1, x2, x3] => [x0]`
        self.emit(masm::Instruction::MovDn3, span);
        self.emit_n(
            3,
            Self::assertz_with_message_inst("128-bit value does not fit in u32", span),
            span,
        );
    }

    /// Convert a unsigned 128-bit value to i64
    ///
    /// This is different than `trunc_i128_to_i64`, as this function performs a
    /// range check on the input value to ensure that it will fit in a i64.
    ///
    /// This consumes the input value, and leaves an i64 value on the stack.
    ///
    /// NOTE: This function does not validate the i128, the caller is expected to
    /// have already validated that the top of the stack holds a valid i128.
    pub fn u128_to_i64(&mut self, span: SourceSpan) {
        // Drop the most significant 64 bits, so long as those bits are zero
        self.int128_to_u64(span);
        // Ensure that the remaining 64 bits are a valid non-negative i64 value
        self.assert_unsigned_int64(span);
    }

    /// Convert an i128 value to i64
    ///
    /// This is different than `trunc_i128_to_i64`, as this function performs a
    /// range check on the input value to ensure that it will fit in a i64.
    ///
    /// This consumes the input value, and leaves an i64 value on the stack.
    ///
    /// NOTE: This function does not validate the i128, the caller is expected to
    /// have already validated that the top of the stack holds a valid i128.
    pub fn i128_to_i64(&mut self, span: SourceSpan) {
        // The value fits in an i64 if its most significant 64 bits extend the sign of the low
        // half, the most significant bit of `x1`: both high limbs are all ones if it is set, and
        // all zeros if it is not.
        //
        // [x1, x0, x1, x2, x3]
        self.emit(masm::Instruction::Dup1, span);
        // [is_signed, x0, x1, x2, x3]
        self.const_mask_u32(SIGN_BIT, span);
        self.emit(masm::Instruction::EqImm(Felt::new_unchecked(SIGN_BIT as u64).into()), span);
        // Select the expected value of each high limb based on the is_signed flag
        //
        // [expected, x0, x1, x2, x3]
        self.select_int32(u32::MAX, 0, span);
        self.emit_all(
            [
                // [x2, expected, x0, x1, x3]
                masm::Instruction::MovUp3,
                // [expected, x2, expected, x0, x1, x3]
                masm::Instruction::Dup1,
                // [expected, x0, x1, x3]
                Self::assert_eq_with_message_inst("128-bit value does not fit in i64", span),
                // [x3, expected, x0, x1]
                masm::Instruction::MovUp3,
                // [x0, x1]
                Self::assert_eq_with_message_inst("128-bit value does not fit in i64", span),
            ],
            span,
        );
    }

    /// Truncate this i128 value to a felt value
    ///
    /// This consumes the input value, and leaves a felt value on the stack.
    ///
    /// NOTE: This function does not validate the i128, that is left up to the caller.
    #[inline]
    pub fn trunc_i128_to_felt(&mut self, span: SourceSpan) {
        // Drop the most significant 64 bits, then truncate the low half
        self.trunc_i128(64, span);
        self.trunc_int64_to_felt(span);
    }

    /// Truncate this i128 value to N bits, where N is <= 64
    ///
    /// This consumes the input value, and leaves an N-bit value on the stack,
    /// where the value is assumed to be represented using 32-bit limbs.
    /// For example, a 64-bit value will consist of two 32-bit values on the
    /// stack.
    ///
    /// NOTE: This function does not validate the i128 value, that is left up to the caller.
    #[inline]
    pub fn trunc_i128(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 64);
        match n {
            // Drop the two most significant limbs: `[x0, x1, x2, x3] => [x0, x1]`
            64 => {
                self.emit_all(
                    [
                        masm::Instruction::MovUp3,
                        masm::Instruction::Drop,
                        masm::Instruction::MovUp2,
                        masm::Instruction::Drop,
                    ],
                    span,
                );
            }
            // Move the least significant limb below the three others, and drop them:
            // `[x0, x1, x2, x3] => [x0]`
            n => {
                self.emit(masm::Instruction::MovDn3, span);
                self.emit_n(3, masm::Instruction::Drop, span);
                match n {
                    32 => (),
                    n => self.trunc_int32(n, span),
                }
            }
        }
    }

    /// Pop two i128 values, `b` and `a`, off the operand stack, and place the result of `a == b` on
    /// the stack.
    #[inline]
    pub fn eq_i128(&mut self, span: SourceSpan) {
        self.emit_all(
            [
                masm::Instruction::Eqw,
                // Move the boolean below the elements we're going to drop
                masm::Instruction::MovDn8,
                // Drop both i128 values
                masm::Instruction::DropW,
                masm::Instruction::DropW,
            ],
            span,
        );
    }

    /// Pop two i128 values, `b` and `a`, off the operand stack, and place the result of `a == b` on
    /// the stack.
    #[inline]
    pub fn neq_i128(&mut self, span: SourceSpan) {
        self.eq_i128(span);
        self.emit(masm::Instruction::Not, span);
    }

    /// Pop two i128 values off the stack, `b` and `a`, and place the result of  `a + b` on the
    /// stack.
    ///
    /// The core library takes its operands in this order, `b` on top: `[b0, b1, b2, b3, a0, a1,
    /// a2, a3]`.
    ///
    /// Wrapping addition does not depend on signedness, so this serves `u128` values too. For now
    /// only wrapping addition is supported.
    #[inline]
    pub fn add_i128(&mut self, overflow: Overflow, span: SourceSpan) {
        assert!(
            matches!(overflow, Overflow::Wrapping),
            "Only 128bit *wrapping* adds implemented as yet."
        );
        self.raw_exec("::miden::core::math::u128::wrapping_add", span);
    }

    /// Pops two i128 values off the stack, `b` and `a`, and performs `a - b`.
    ///
    /// Wrapping subtraction does not depend on signedness, so this serves `u128` values too. For
    /// now only wrapping subtraction is supported.
    pub fn sub_i128(&mut self, overflow: Overflow, span: SourceSpan) {
        assert!(
            matches!(overflow, Overflow::Wrapping),
            "Only 128bit *wrapping* subs implemented as yet."
        );
        self.raw_exec("::miden::core::math::u128::wrapping_sub", span);
    }

    /// Pops two i128 values off the stack, `b` and `a`, and performs `a * b`.
    #[inline]
    pub fn mul_i128(&mut self, span: SourceSpan) {
        self.raw_exec("::miden::core::math::u128::wrapping_mul", span);
    }
}
