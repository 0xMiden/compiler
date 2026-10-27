use miden_core::Felt;
use midenc_hir::{Overflow, SourceSpan};

use super::{OpEmitter, int32::SIGN_BIT, masm};

/// The value zero, as a field element
pub const ZERO: Felt = Felt::ZERO;

/// The value 2^32, as a field element
pub const U32_FIELD_MODULUS: Felt = Felt::new_unchecked(2u64.pow(32));

#[allow(unused)]
impl OpEmitter<'_> {
    /// Add the two canonical Felt operands at the top of the raw stack.
    ///
    /// Checked mode traps when canonical addition reaches the field modulus; overflowing mode
    /// leaves `[overflow_flag, field_result | suffix]`, top first. Wrapping and unchecked modes
    /// emit ordinary field arithmetic. This helper emits instructions only: its typed caller owns
    /// operand-stack bookkeeping, and the suffix below the two inputs is preserved.
    pub fn add_felt(&mut self, overflow: Overflow, span: SourceSpan) {
        use masm::Instruction as I;
        if matches!(overflow, Overflow::Unchecked | Overflow::Wrapping) {
            self.emit(I::Add, span);
            return;
        }
        self.emit_all(
            [
                // [r, b, a | S]
                I::Dup1,
                I::Dup1,
                I::Add,
                // At most one reduction: r < a iff a+b >= p. Compare full felts.
                I::Dup0,
                I::Dup3,
                I::Lt,
                // [flag, r | S]
                I::MovUp2,
                I::Drop,
                I::MovUp2,
                I::Drop,
            ],
            span,
        );
        self.finish_felt_arithmetic("add", overflow, span);
    }

    /// Subtract the two canonical Felt operands at the top of the raw stack.
    ///
    /// Checked mode traps when the left operand is smaller than the right operand; overflowing mode
    /// leaves `[overflow_flag, field_result | suffix]`, top first. Wrapping and unchecked modes
    /// emit ordinary field arithmetic. This helper emits instructions only: its typed caller owns
    /// operand-stack bookkeeping, and the suffix below the two inputs is preserved.
    pub fn sub_felt(&mut self, overflow: Overflow, span: SourceSpan) {
        use masm::Instruction as I;
        if matches!(overflow, Overflow::Unchecked | Overflow::Wrapping) {
            self.emit(I::Sub, span);
            return;
        }
        self.emit_all([I::Dup1, I::Dup1, I::Lt, I::MovDn2, I::Sub, I::Swap1], span);
        self.finish_felt_arithmetic("sub", overflow, span);
    }

    /// Multiply the two canonical Felt operands at the top of the raw stack.
    ///
    /// Checked mode traps when the canonical integer product reaches the field modulus; overflowing mode
    /// leaves `[overflow_flag, field_result | suffix]`, top first. Wrapping and unchecked modes
    /// emit ordinary field arithmetic. This helper emits instructions only: its typed caller owns
    /// operand-stack bookkeeping, and the suffix below the two inputs is preserved.
    pub fn mul_felt(&mut self, overflow: Overflow, span: SourceSpan) {
        use masm::Instruction as I;
        if matches!(overflow, Overflow::Unchecked | Overflow::Wrapping) {
            self.emit(I::Mul, span);
            return;
        }
        self.emit_all(
            [
                // Retain the native field result below the two split operands.
                I::Dup1,
                I::Dup1,
                I::Mul,
                I::MovDn2,
                I::U32Split,
                I::MovUp2,
                I::U32Split,
            ],
            span,
        );
        self.raw_exec("::miden::core::math::u64::widening_mul", span);
        // [p0,p1,p2,p3,r | S]. Product >= p iff its upper64 is nonzero, or
        // p1 == MAX32 and p0 != 0. In particular 2^64-1 requires reduction.
        self.emit_all(
            [
                I::NeqImm(Felt::ZERO.into()),
                I::Swap1,
                I::EqImm(Felt::new_unchecked(u32::MAX as u64).into()),
                I::And,
                I::Swap1,
                I::NeqImm(Felt::ZERO.into()),
                I::Or,
                I::Swap1,
                I::NeqImm(Felt::ZERO.into()),
                I::Or,
            ],
            span,
        );
        self.finish_felt_arithmetic("mul", overflow, span);
    }

    fn finish_felt_arithmetic(&mut self, op: &str, overflow: Overflow, span: SourceSpan) {
        if overflow == Overflow::Checked {
            self.emit(Self::assertz_with_message_inst(format!("felt {op} overflow"), span), span);
        }
    }

    /// This operation checks if the field element on top of the stack is zero.
    ///
    /// This operation does not consume the input, and pushes a boolean value on the stack.
    ///
    /// # Stack effects
    ///
    /// `[a, ..] => [a == 0, a, ..]`
    #[inline(always)]
    pub fn felt_is_zero(&mut self, span: SourceSpan) {
        self.emit_all([masm::Instruction::Dup0, masm::Instruction::EqImm(ZERO.into())], span);
    }

    /// This operation asserts the field element on top of the stack is zero.
    ///
    /// This operation does not consume the input.
    ///
    /// # Stack effects
    ///
    /// `[a, ..] => [a, ..]`
    #[inline(always)]
    pub fn assert_felt_is_zero(&mut self, span: SourceSpan) {
        self.emit_all(
            [
                masm::Instruction::Dup0,
                Self::assertz_with_message_inst("expected felt value to be zero", span),
            ],
            span,
        );
    }

    /// Convert a field element to i128 by zero-extension.
    ///
    /// This consumes the field element on top of the stack.
    ///
    /// # Stack effects
    ///
    /// `[a, ..] => [a_lo, a_hi, 0, 0]`
    #[inline]
    pub fn felt_to_i128(&mut self, span: SourceSpan) {
        // [a_lo, a_hi]
        self.emit(masm::Instruction::U32Split, span);
        // [0, 0, a_lo, a_hi]
        self.emit_push(ZERO, span);
        self.emit_push(ZERO, span);
        // Move the limbs of `a` back above the most significant limbs: [a_lo, a_hi, 0, 0]
        self.emit_all([masm::Instruction::MovUp3, masm::Instruction::MovUp3], span);
    }

    /// Convert a field element to u64 by zero-extension.
    ///
    /// This consumes the field element on top of the stack.
    ///
    /// # Stack effects
    ///
    /// `[a, ..] => [a_lo, a_hi]`
    #[inline(always)]
    pub fn felt_to_u64(&mut self, span: SourceSpan) {
        self.emit(masm::Instruction::U32Split, span);
    }

    /// Convert a field element to i64 by zero-extension.
    ///
    /// Asserts if the field element is too large to represent as an i64, that is if it is at or
    /// above 2^63.
    ///
    /// This consumes the field element on top of the stack.
    ///
    /// # Stack effects
    ///
    /// `[a, ..] => [a_lo, a_hi]`
    #[inline]
    pub fn felt_to_i64(&mut self, span: SourceSpan) {
        self.felt_to_u64(span);
        // The felt fits exactly when bit 63, the sign bit of the high limb, is clear
        self.emit(masm::Instruction::Dup1, span);
        self.const_mask_u32(SIGN_BIT, span);
        self.emit(Self::assertz_with_message_inst("felt value does not fit in i64", span), span);
    }

    /// Convert a field element value to an unsigned N-bit integer, where N <= 32
    ///
    /// Conversion will trap if the input value is too large to fit in an unsigned N-bit integer.
    pub fn felt_to_uint(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        self.emit_all(
            [
                // Split into u32 limbs: [a_lo, a_hi]
                masm::Instruction::U32Split,
                // Assert the most significant 32 bits, the high limb below the low one, are unused
                masm::Instruction::Swap1,
                Self::assertz_with_message_inst("felt value does not fit in 32 bits", span),
            ],
            span,
        );
        if n < 32 {
            // Convert to N-bit integer
            self.int32_to_uint(n, span);
        }
    }

    /// Convert a field element value to a signed N-bit integer, where N <= 32
    ///
    /// Conversion will trap if the input value is too large to fit in a signed N-bit integer.
    pub fn felt_to_int(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        self.emit_all(
            [
                // Split into u32 limbs: [a_lo, a_hi]
                masm::Instruction::U32Split,
                // Assert the most significant 32 bits, the high limb below the low one, are unused
                masm::Instruction::Swap1,
                Self::assertz_with_message_inst("felt value does not fit in 32 bits", span),
            ],
            span,
        );
        if n < 32 {
            // A felt is never negative, so it fits in a signed N-bit integer exactly when it is
            // below 2^(N-1)
            self.int32_to_uint(n, span);
            self.assert_unsigned_smallint(n, span);
        } else {
            // Assert the sign bit isn't set
            self.assert_unsigned_int32(span);
        }
    }

    /// Zero-extend a field element value to N-bits, where N >= 64
    ///
    /// N must be a power of two, or this function will panic.
    pub fn zext_felt(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 64, 256);
        match n {
            64 => self.felt_to_u64(span),
            128 => self.felt_to_i128(span),
            n => {
                // Convert to u64 and zero-extend
                self.felt_to_u64(span);
                self.zext_int64(n, span);
            }
        }
    }

    /// Emits code to sign-extend a field element value to an N-bit integer, where N >= 64
    ///
    /// Field elements are unsigned, so sign-extension here is indicating that the target
    /// integer type is a signed type, so we have one less bit available to use.
    ///
    /// This does not check that the felt fits in the signed type: `cast` does that, with
    /// [Self::felt_to_i64].
    ///
    /// N must be a power of two, or this function will panic.
    pub fn sext_felt(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 64, 256);
        match n {
            64 => self.felt_to_u64(span),
            128 => self.felt_to_i128(span),
            n => {
                // A field element is never negative: convert to u64 and zero-extend
                self.felt_to_u64(span);
                self.zext_int64(n, span);
            }
        }
    }

    /// Truncates a field element on top of the stack to an N-bit integer, where N <= 32.
    ///
    /// Truncation on field elements is not well-defined, because field elements do not have
    /// a specified bitwise representation. To implement semantics equivalent to the other types
    /// which _do_ have a specified representation, we first convert the input field element to u32,
    /// and then masking out any additional unused bits of the u32 representation.
    ///
    /// This should produce outputs which are identical to equivalent u64 values, i.e. the same
    /// value in both u64 and felt representation will be truncated to the same u32 value.
    #[inline]
    pub fn trunc_felt(&mut self, n: u32, span: SourceSpan) {
        // Apply a field modulus of 2^32, i.e. `a mod 2^32`, converting
        // the field element into the u32 range. Miden defines values in
        // this range as having a standard unsigned binary representation.
        self.emit(masm::Instruction::U32Cast, span);
        self.trunc_int32(n, span);
    }

    /// Make `n` copies of the element on top of the stack
    #[inline(always)]
    pub fn dup_felt(&mut self, count: u8, span: SourceSpan) {
        self.emit_n(count as usize, masm::Instruction::Dup0, span);
    }
}
