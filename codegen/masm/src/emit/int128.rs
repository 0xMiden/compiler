use midenc_hir::{Overflow, SourceSpan};

use super::{OpEmitter, masm};

#[allow(unused)]
impl OpEmitter<'_> {
    /// Checks if the i128 value on the stack has its sign bit set.
    #[inline(always)]
    pub fn is_signed_int128(&mut self, span: SourceSpan) {
        self.is_signed_int32(span)
    }

    /// Assert that the i128 value on the stack does not have its sign bit set.
    #[inline(always)]
    pub fn assert_unsigned_int128(&mut self, span: SourceSpan) {
        // Assert that the sign bit is unset
        self.assert_unsigned_int32(span)
    }

    /// Push a u128 value on the operand stack
    ///
    /// An u128 value consists of 4 32-bit limbs
    pub fn push_u128(&mut self, value: u128, span: SourceSpan) {
        let bytes = value.to_le_bytes();
        let hi = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let lo = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        self.push_u64(lo, span);
        self.push_u64(hi, span);
    }

    /// Push an i128 value on the operand stack
    ///
    /// An i128 value consists of 4 32-bit limbs
    pub fn push_i128(&mut self, value: i128, span: SourceSpan) {
        let bytes = value.to_le_bytes();
        let hi = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let lo = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        self.push_u64(lo, span);
        self.push_u64(hi, span);
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
        // Assert the first two limbs are equal to 0
        //
        // What remains on the stack at this point are the low 64-bits,
        // which is also our result.
        self.emit_n(2, masm::Instruction::Assertz, span);
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
        // Assert the first three limbs are equal to 0
        //
        // What remains on the stack at this point are the low 32-bits,
        // which is also our result.
        self.emit_n(3, masm::Instruction::Assertz, span);
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
        // Truncate the first 64-bits, so long as those bits are zero
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
        // Determine if this value is signed or not
        self.is_signed_int32(span);
        // Preserving the is_signed flag, select the expected hi bits value
        self.emit(masm::Instruction::Dup0, span);
        self.select_int32(u32::MAX, 0, span);
        // Move the most significant 64 bits to top of stack
        self.move_int64_up(2, span);
        // Move expected value to top of stack
        self.emit(masm::Instruction::MovUp2, span);
        // Assert the most significant 32 bits match, without consuming them
        self.assert_eq_u32(span);
        self.emit_all(
            [
                // Assert that both 32-bit limbs of the most significant 64 bits match,
                // consuming them in the process
                masm::Instruction::AssertEq,
                // At this point, the stack is: [is_signed, x1, x0]
                //
                // Select an expected value for the sign bit based on the is_signed flag
                masm::Instruction::Swap1,
            ],
            span,
        );
        // [is_sign_bit_set, x1, is_signed, x0]
        self.is_const_flag_set_u32(1 << 31, span);
        self.emit_all(
            [
                // [is_signed, is_sign_bit_set, x1, x0]
                masm::Instruction::MovUp2,
                // Assert that the flags are equal: either the input was signed and the
                // sign bit was set, or the input was unsigned, and the sign bit was unset,
                // any other combination will trap.
                //
                // [x1, x0]
                masm::Instruction::AssertEq,
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
        self.emit_n(2, masm::Instruction::Drop, span);
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
            64 => {
                self.emit_n(2, masm::Instruction::Drop, span);
            }
            32 => {
                self.emit_n(3, masm::Instruction::Drop, span);
            }
            n => {
                self.trunc_int32(n, span);
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
    /// For now we're only supporting wrapping add for signed values.
    #[inline]
    pub fn add_i128(&mut self, overflow: Overflow, span: SourceSpan) {
        assert!(
            matches!(overflow, Overflow::Wrapping),
            "Only 128bit *wrapping* adds implemented as yet."
        );

        self.raw_exec("intrinsics::i128::add", span);
    }

    /// Pops two i128 values off the stack, `b` and `a`, and performs `a - b`.
    pub fn sub_i128(&mut self, overflow: Overflow, span: SourceSpan) {
        assert!(
            matches!(overflow, Overflow::Wrapping),
            "Only 128bit *wrapping* subs implemented as yet."
        );

        self.raw_exec("intrinsics::i128::sub", span);
    }

    /// Pops two i128 values off the stack, `b` and `a`, and performs `a * b`.
    #[inline]
    pub fn mul_i128(&mut self, span: SourceSpan) {
        self.raw_exec("intrinsics::i128::mul", span);
    }
}
