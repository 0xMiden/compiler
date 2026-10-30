//! A "smallint" is an `N`-bit, signed or unsigned integer in two's complement representation,
//! where `N` is defined as being 32 bits or smaller, and a power of two.
//!
//! In Miden, unsigned smallint operations are largely handled with the native u32 operations,
//! however we perform range checks on checked operations to ensure the value is still a valid
//! `N`-bit integer.
//!
//! For signed smallint operations, we implement them in terms of a two's complement representation,
//! using a set of common primitives. The only thing that changes are which bits are considered by
//! those primitives.
use midenc_hir::{Overflow, SourceSpan};

use super::{OpEmitter, masm, movup_from_offset};

#[allow(unused)]
impl OpEmitter<'_> {
    /// Check that a u32 value on the stack can fit in the unsigned N-bit integer range
    #[inline(always)]
    pub fn is_valid_uint(&mut self, n: u32, span: SourceSpan) {
        // Use fallible conversion from u32
        self.try_int32_to_uint(n, span);
    }

    /// Check that the 32-bit value on the stack can fit in the signed N-bit integer range
    #[inline(always)]
    pub fn is_valid_int(&mut self, n: u32, span: SourceSpan) {
        self.try_int32_to_int(n, span);
    }

    /// Check if the sign bit of an N-bit integer on the stack, is set.
    #[inline]
    pub fn is_signed_smallint(&mut self, n: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        match n {
            // i1 is never signed
            1 => self.emit_push(0u32, span),
            n => self.is_const_flag_set_u32(1 << (n - 1), span),
        }
    }

    /// Asserts the N-bit integer on the stack does not have its sign bit set.
    #[inline]
    pub fn assert_unsigned_smallint(&mut self, n: u32, span: SourceSpan) {
        match n {
            // i1 is always unsigned
            1 => (),
            n => {
                self.is_signed_smallint(n, span);
                self.emit(
                    Self::assertz_with_message_inst(
                        format!("{n}-bit integer signedness check failed"),
                        span,
                    ),
                    span,
                );
            }
        }
    }

    /// Convert a signed N-bit integer to a field element
    #[inline(always)]
    pub fn int_to_felt(&mut self, n: u32, span: SourceSpan) {
        self.assert_unsigned_smallint(n, span);
    }

    /// Convert an unsigned N-bit integer to a field element
    #[inline(always)]
    pub fn uint_to_felt(&mut self, n: u32, _span: SourceSpan) {
        // Conversion to felt is a no-op
        assert_valid_integer_size!(n, 1, 32);
    }

    /// Convert a signed N-bit integer to u64
    ///
    /// This operation will trap if the value has the sign bit set.
    #[inline]
    pub fn int_to_u64(&mut self, n: u32, span: SourceSpan) {
        self.assert_unsigned_smallint(n, span);
        self.zext_smallint(n, 64, span);
    }

    /// Convert an unsigned N-bit integer to u64
    #[inline(always)]
    pub fn uint_to_u64(&mut self, n: u32, span: SourceSpan) {
        self.zext_smallint(n, 64, span);
    }

    /// Convert a signed N-bit integer to i128
    #[inline]
    pub fn int_to_i128(&mut self, n: u32, span: SourceSpan) {
        self.sext_smallint(n, 128, span);
    }

    /// Convert an unsigned N-bit integer to i128
    #[inline(always)]
    pub fn uint_to_i128(&mut self, n: u32, span: SourceSpan) {
        self.zext_smallint(n, 128, span);
    }

    /// Sign-extend the N-bit value on the stack to M-bits, where M is >= N and <= 256.
    ///
    /// This assumes the value on the stack is a valid N-bit integer in two's complement
    /// representation. I1 is always unsigned; other widths use their most significant bit.
    pub fn sext_smallint(&mut self, n: u32, m: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        assert_valid_integer_size!(m, n, 256);
        // No-op
        if n == m {
            return;
        }
        // Extend only through the destination pattern when it occupies a partial limb.
        let num_sign_bits = m.min(32) - n;
        // Fill the destination bits above N without exposing bits beyond a narrow result.
        let sign_bits = if num_sign_bits == 0 {
            0
        } else {
            ((1u32 << num_sign_bits) - 1) << n
        };
        // We optimize larger extensions by re-using the is_signed flag
        let is_large = m > 32;
        // Get the value of the sign bit
        self.is_signed_smallint(n, span);
        if is_large {
            // Make a copy for selecting padding later
            self.emit(masm::Instruction::Dup0, span);
            self.select_int32(sign_bits, 0, span);
            self.emit_all(
                [
                    // Move the input value to the top of the stack
                    masm::Instruction::MovUp2,
                    // Sign-extend to i32
                    masm::Instruction::U32Or,
                    // Move the is_signed flag back to the top
                    masm::Instruction::Swap1,
                ],
                span,
            );
            // Select the padding element value
            self.select_int32(u32::MAX, 0, span);
            // Pad out to M bits
            self.pad_int32(m, span);
            // Multi-limb integers are represented in little-endian limb order on the operand
            // stack, i.e. the least-significant limb is on top. After selecting the padding limb
            // and extending to i32, the sign-extended payload limb sits below the padding limbs, so
            // move it back to the top.
            let num_elements = m / 32;
            match num_elements {
                2 => self.emit(masm::Instruction::Swap1, span),
                n => self.emit(movup_from_offset((n - 1) as usize), span),
            }
        } else {
            self.select_int32(sign_bits, 0, span);
            // Extend within the destination's single-lane pattern.
            self.emit(masm::Instruction::U32Or, span);
        }
    }

    /// Zero-extend the N-bit small integer on the stack to M-bits, where N <= 32 and
    /// N <= M <= 256.
    ///
    /// This assumes the value on the stack is a valid N-bit integer.
    #[inline]
    pub fn zext_smallint(&mut self, n: u32, m: u32, span: SourceSpan) {
        assert_valid_integer_size!(n, 1, 32);
        assert_valid_integer_size!(m, n, 256);
        // A valid small integer already has zeroes above its N-bit pattern in this lane.
        if m <= 32 {
            return;
        }
        self.zext_int32(m, span);
    }

    pub fn add_uint(&mut self, n: u32, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked => self.add_u32(Overflow::Unchecked, span),
            overflow => {
                self.add_u32(Overflow::Checked, span);
                self.handle_uint_overflow(n, overflow, span)
            }
        }
    }

    pub fn add_imm_uint(&mut self, imm: u32, n: u32, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked => self.add_imm_u32(imm, Overflow::Unchecked, span),
            overflow => {
                self.add_imm_u32(imm, Overflow::Checked, span);
                self.handle_uint_overflow(n, overflow, span)
            }
        }
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a - b`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    pub fn sub_uint(&mut self, n: u32, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked => self.sub_u32(overflow, span),
            Overflow::Overflowing => {
                // Underflow wraps outside the N-bit range. Let the width check produce the
                // flag from that result, rather than passing it a u32 borrow flag.
                self.sub_u32(Overflow::Wrapping, span);
                self.handle_uint_overflow(n, overflow, span);
            }
            overflow => {
                self.sub_u32(overflow, span);
                self.handle_uint_overflow(n, overflow, span);
            }
        }
    }

    /// Pops a u32 value off the stack, `a`, and performs `a - <imm>`.
    ///
    /// See the [Overflow] type for how overflow semantics can change the operation.
    ///
    /// Subtracting zero preserves the input and emits a false flag for overflowing arithmetic.
    #[inline]
    pub fn sub_imm_uint(&mut self, imm: u32, n: u32, overflow: Overflow, span: SourceSpan) {
        if imm == 0 {
            self.emit_no_overflow_flag(overflow, span);
            return;
        }
        match overflow {
            Overflow::Unchecked => self.sub_imm_u32(imm, overflow, span),
            Overflow::Overflowing => {
                self.sub_imm_u32(imm, Overflow::Wrapping, span);
                self.handle_uint_overflow(n, overflow, span);
            }
            overflow => {
                self.sub_imm_u32(imm, overflow, span);
                self.handle_uint_overflow(n, overflow, span);
            }
        }
    }

    pub fn mul_uint(&mut self, n: u32, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked => self.mul_u32(Overflow::Unchecked, span),
            overflow => {
                self.mul_u32(Overflow::Checked, span);
                self.handle_uint_overflow(n, overflow, span)
            }
        }
    }

    pub fn mul_imm_uint(&mut self, imm: u32, n: u32, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked => self.mul_imm_u32(imm, Overflow::Unchecked, span),
            overflow => {
                self.mul_imm_u32(imm, Overflow::Checked, span);
                self.handle_uint_overflow(n, overflow, span)
            }
        }
    }

    #[inline]
    pub fn checked_div_uint(&mut self, n: u32, span: SourceSpan) {
        self.checked_div_u32(span);
        self.int32_to_uint(n, span);
    }

    #[inline]
    pub fn checked_div_imm_uint(&mut self, imm: u32, n: u32, span: SourceSpan) {
        self.checked_div_imm_u32(imm, span);
        self.int32_to_uint(n, span);
    }

    #[inline(always)]
    pub fn unchecked_div_uint(&mut self, _n: u32, span: SourceSpan) {
        self.unchecked_div_u32(span);
    }

    #[inline(always)]
    pub fn unchecked_div_imm_uint(&mut self, imm: u32, _n: u32, span: SourceSpan) {
        self.unchecked_div_imm_u32(imm, span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a % b`.
    ///
    /// Emits [Self::checked_mod_u32], with the same requirement on its operands.
    #[inline(always)]
    pub fn checked_mod_uint(&mut self, _n: u32, span: SourceSpan) {
        self.checked_mod_u32(span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a % <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    ///
    /// Emits [Self::checked_mod_imm_u32], with the same requirement on its operand.
    #[inline(always)]
    pub fn checked_mod_imm_uint(&mut self, imm: u32, _n: u32, span: SourceSpan) {
        self.checked_mod_imm_u32(imm, span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and performs `a % b`.
    ///
    /// Emits [Self::unchecked_mod_u32], with the same requirement on its operands; not checked to
    /// fit in `n` bits.
    #[inline(always)]
    pub fn unchecked_mod_uint(&mut self, _n: u32, span: SourceSpan) {
        self.unchecked_mod_u32(span);
    }

    /// Pops a u32 value off the stack, `a`, and performs `a % <imm>`.
    ///
    /// This function will panic if the divisor is zero.
    ///
    /// Emits [Self::unchecked_mod_imm_u32], with the same requirement on its operand; not checked
    /// to fit in `n` bits.
    #[inline(always)]
    pub fn unchecked_mod_imm_uint(&mut self, imm: u32, _n: u32, span: SourceSpan) {
        self.unchecked_mod_imm_u32(imm, span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and pushes `a / b`, then `a % b` on the
    /// stack.
    ///
    /// Emits [Self::checked_divmod_u32], with the same requirement on its operands.
    #[inline(always)]
    pub fn checked_divmod_uint(&mut self, _n: u32, span: SourceSpan) {
        self.checked_divmod_u32(span);
    }

    /// Pops a u32 value off the stack, `a`, and pushes `a / <imm>`, then `a % <imm>` on the stack.
    ///
    /// Emits [Self::checked_divmod_imm_u32], with the same requirement on its operand.
    #[inline(always)]
    pub fn checked_divmod_imm_uint(&mut self, imm: u32, _n: u32, span: SourceSpan) {
        self.checked_divmod_imm_u32(imm, span);
    }

    /// Pops two u32 values off the stack, `b` and `a`, and pushes `a / b`, then `a % b` on the
    /// stack.
    ///
    /// Emits [Self::unchecked_divmod_u32], with the same requirement on its operands; not checked
    /// to fit in `n` bits.
    #[inline(always)]
    pub fn unchecked_divmod_uint(&mut self, _n: u32, span: SourceSpan) {
        self.unchecked_divmod_u32(span);
    }

    /// Pops a u32 value off the stack, `a`, and pushes `a / <imm>`, then `a % <imm>` on the stack.
    ///
    /// This function will panic if the divisor is zero.
    ///
    /// Emits [Self::unchecked_divmod_imm_u32], with the same requirement on its operand; not
    /// checked to fit in `n` bits.
    #[inline(always)]
    pub fn unchecked_divmod_imm_uint(&mut self, imm: u32, _n: u32, span: SourceSpan) {
        self.unchecked_divmod_imm_u32(imm, span)
    }

    pub fn handle_uint_overflow(&mut self, n: u32, overflow: Overflow, span: SourceSpan) {
        match overflow {
            Overflow::Unchecked => (),
            Overflow::Checked => self.int32_to_uint(n, span),
            Overflow::Wrapping => {
                self.emit_push(2u32.pow(n), span);
                self.emit(masm::Instruction::U32Mod, span);
            }
            Overflow::Overflowing => {
                self.try_int32_to_uint(n, span);
                // move result to top, and wrap it at 2^n
                self.emit(masm::Instruction::Swap1, span);
                self.emit_push(2u32.pow(n), span);
                self.emit_all(
                    [
                        masm::Instruction::U32Mod,
                        // move is_valid flag to top, and invert it
                        masm::Instruction::Swap1,
                        masm::Instruction::Not,
                    ],
                    span,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{collections::BTreeSet, rc::Rc, string::String, vec::Vec};

    use midenc_hir::{Context, Report, formatter::PrettyPrint};

    use super::*;
    use crate::OperandStack;

    /// Invoke the raw shared owner and check every result limb and a suffix wider than the
    /// indexed window, independently of the emitter's typed operand bookkeeping.
    fn check_extension(
        bits: u32,
        value: u32,
        emit: impl FnOnce(&mut OpEmitter<'_>, SourceSpan),
    ) -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let mut stack = OperandStack::new(context);
        let mut invoked = BTreeSet::new();
        let mut body = Vec::new();
        let span = SourceSpan::UNKNOWN;
        {
            let mut emitter = OpEmitter::new(&mut invoked, &mut body, &mut stack);
            for i in (0..20).rev() {
                emitter.literal(1000u32 + i, span);
            }
            emitter.literal(value, span);
            emit(&mut emitter, span);
        }
        let assertions = core::iter::once(value)
            .chain(core::iter::repeat_n(0, bits.div_ceil(32) as usize - 1))
            .chain(1000..1020)
            .map(|lane| format!("push.{lane} assert_eq\n"))
            .collect::<String>();
        let code =
            format!("begin {} {assertions} end", masm::Block::new(span, body).to_pretty_string());
        let program = miden_assembly::Assembler::default()
            .assemble_program("smallint_extension", code)?
            .try_into_program()?;
        let mut host = crate::plan::tests::lowering_units::host();
        miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
            .execute_sync(&program, &mut host)
            .map_err(|e| Report::msg(e.to_string()))?;
        Ok(())
    }

    #[test]
    fn zero_extension_width_matrix_preserves_value_limbs_and_suffix() -> Result<(), Report> {
        for n in [1, 8, 16, 32] {
            for m in [1, 8, 16, 32, 64, 128, 256].into_iter().filter(|m| *m >= n) {
                for value in [0, ((1u64 << n) - 1) as u32] {
                    check_extension(m, value, |emitter, span| emitter.zext_smallint(n, m, span))?;
                }
            }
        }
        Ok(())
    }

    #[test]
    fn wider_direct_callers_preserve_value_limbs_and_suffix() -> Result<(), Report> {
        for n in [8, 16] {
            for value in [0, (1u32 << n) - 1] {
                check_extension(64, value, |emitter, span| emitter.uint_to_u64(n, span))?;
                check_extension(128, value, |emitter, span| emitter.uint_to_i128(n, span))?;
            }
            for value in [0, (1u32 << (n - 1)) - 1] {
                check_extension(64, value, |emitter, span| emitter.int_to_u64(n, span))?;
            }
        }
        Ok(())
    }

    #[test]
    fn zero_extension_rejects_invalid_widths() {
        for (n, m) in [(0, 32), (3, 32), (64, 64), (8, 3), (16, 8), (8, 512)] {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let context = Rc::new(Context::default());
                let mut stack = OperandStack::new(context);
                let mut invoked = BTreeSet::new();
                let mut body = Vec::new();
                let mut emitter = OpEmitter::new(&mut invoked, &mut body, &mut stack);
                emitter.zext_smallint(n, m, SourceSpan::UNKNOWN);
            }));
            assert!(result.is_err(), "invalid extension {n} -> {m}");
        }
    }
}
