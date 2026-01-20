use midenc_hir::{
    Builder, BuilderExt, Felt, OpBuilder, Overflow, Report, SourceSpan, Type, ValueRef,
    dialects::builtin::FunctionBuilder,
};

use crate::*;

pub trait ArithOpBuilder<'f, B: ?Sized + Builder> {
    /*
    fn character(&mut self, c: char, span: SourceSpan) -> Value {
        self.i32((c as u32) as i32, span)
    }
    */

    fn i1(&mut self, value: bool, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I1(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn i8(&mut self, value: i8, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I8(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn i16(&mut self, value: i16, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I16(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn i32(&mut self, value: i32, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I32(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn i64(&mut self, value: i64, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::I64(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn u8(&mut self, value: u8, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::U8(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn u16(&mut self, value: u16, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::U16(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn u32(&mut self, value: u32, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::U32(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn u64(&mut self, value: u64, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::U64(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn f64(&mut self, value: f64, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::F64(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn felt(&mut self, value: Felt, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(Immediate::Felt(value)).unwrap();
        constant.borrow().result().as_value_ref()
    }

    fn imm(&mut self, value: Immediate, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<crate::ops::Constant, _>(span);
        let constant = op_builder(value).unwrap();
        constant.borrow().result().as_value_ref()
    }

    /// Truncates an integral value as necessary to fit in `ty`.
    ///
    /// NOTE: Truncating a value into a larger type has undefined behavior, it is
    /// equivalent to extending a value without doing anything with the new high-order
    /// bits of the resulting value.
    fn trunc(&mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Trunc, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Extends an integer into a larger integeral type, by zero-extending the value,
    /// i.e. the new high-order bits of the resulting value will be all zero.
    ///
    /// NOTE: This function will panic if `ty` is smaller than `arg`.
    ///
    /// If `arg` is the same type as `ty`, `arg` is returned as-is
    fn zext(&mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Zext, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Extends an integer into a larger integeral type, by sign-extending the value,
    /// i.e. the new high-order bits of the resulting value will all match the sign bit.
    ///
    /// NOTE: This function will panic if `ty` is smaller than `arg`.
    ///
    /// If `arg` is the same type as `ty`, `arg` is returned as-is
    fn sext(&mut self, arg: ValueRef, ty: Type, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Sext, _>(span);
        let op = op_builder(arg, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement addition which traps on overflow
    fn add(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Add, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement addition. Behavior is undefined if the result overflows.
    fn add_unchecked(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Add, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement addition which wraps around on overflow, e.g. `wrapping_add`
    fn add_wrapping(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Add, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement addition which wraps around on overflow, but returns a boolean flag that
    /// indicates whether or not the operation overflowed, followed by the wrapped result, e.g.
    /// `overflowing_add` (but with the result types inverted compared to Rust's version).
    fn add_overflowing(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::AddOverflowing, _>(span);
        let op = op_builder(lhs, rhs)?;
        let op = op.borrow();
        let overflowed = op.overflowed().as_value_ref();
        let result = op.result().as_value_ref();
        Ok((overflowed, result))
    }

    /// Two's complement subtraction which traps on under/overflow
    fn sub(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Sub, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement subtraction. Behavior is undefined if the result under/overflows.
    fn sub_unchecked(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Sub, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement subtraction which wraps around on under/overflow, e.g. `wrapping_sub`
    fn sub_wrapping(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Sub, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement subtraction which wraps around on overflow, but returns a boolean flag that
    /// indicates whether or not the operation under/overflowed, followed by the wrapped result,
    /// e.g. `overflowing_sub` (but with the result types inverted compared to Rust's version).
    fn sub_overflowing(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::SubOverflowing, _>(span);
        let op = op_builder(lhs, rhs)?;
        let op = op.borrow();
        let overflowed = op.overflowed().as_value_ref();
        let result = op.result().as_value_ref();
        Ok((overflowed, result))
    }

    /// Two's complement multiplication which traps on overflow
    fn mul(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Mul, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement multiplication. Behavior is undefined if the result overflows.
    fn mul_unchecked(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Mul, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement multiplication which wraps around on overflow, e.g. `wrapping_mul`
    fn mul_wrapping(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Mul, _>(span);
        let op = op_builder(lhs, rhs, Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement multiplication which wraps around on overflow, but returns a boolean flag
    /// that indicates whether or not the operation overflowed, followed by the wrapped result,
    /// e.g. `overflowing_mul` (but with the result types inverted compared to Rust's version).
    fn mul_overflowing(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::MulOverflowing, _>(span);
        let op = op_builder(lhs, rhs)?;
        let op = op.borrow();
        let overflowed = op.overflowed().as_value_ref();
        let result = op.result().as_value_ref();
        Ok((overflowed, result))
    }

    /// Integer division. Traps if `rhs` is zero.
    fn div(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Div, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Integer Euclidean modulo. Traps if `rhs` is zero.
    fn r#mod(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Mod, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Combined integer Euclidean division and modulo. Traps if `rhs` is zero.
    fn divmod(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Divmod, _>(span);
        let op = op_builder(lhs, rhs)?;
        let op = op.borrow();
        let quotient = op.quotient().as_value_ref();
        let remainder = op.remainder().as_value_ref();
        Ok((quotient, remainder))
    }

    /// Exponentiation
    fn exp(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Exp, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Compute 2^n
    fn pow2(&mut self, n: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Pow2, _>(span);
        let op = op_builder(n)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Compute ilog2(n)
    fn ilog2(&mut self, n: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Ilog2, _>(span);
        let op = op_builder(n)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Modular inverse
    fn inv(&mut self, n: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Inv, _>(span);
        let op = op_builder(n)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unary negation
    fn neg(&mut self, n: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Neg, _>(span);
        let op = op_builder(n)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement unary increment by one which traps on overflow
    fn incr(&mut self, lhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Incr, _>(span);
        let op = op_builder(lhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Logical AND
    fn and(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::And, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Logical OR
    fn or(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Or, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Logical XOR
    fn xor(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Xor, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Logical NOT
    fn not(&mut self, lhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Not, _>(span);
        let op = op_builder(lhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Bitwise AND
    fn band(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Band, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Bitwise OR
    fn bor(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Bor, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Bitwise XOR
    fn bxor(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Bxor, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Bitwise NOT
    fn bnot(&mut self, lhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Bnot, _>(span);
        let op = op_builder(lhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn rotl(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Rotl, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn rotr(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Rotr, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn shl(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Shl, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn shr(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Shr, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn popcnt(&mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Popcnt, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn clz(&mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Clz, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn ctz(&mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Ctz, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn clo(&mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Clo, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn cto(&mut self, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Cto, _>(span);
        let op = op_builder(rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn eq(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Eq, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn neq(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Neq, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Compares two integers and returns the minimum value
    fn min(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Min, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Compares two integers and returns the maximum value
    fn max(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Max, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn gt(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Gt, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn gte(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Gte, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn lt(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Lt, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn lte(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Lte, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Join `hi` and `lo` into a single value of type `ty`.
    fn join(
        &mut self,
        hi: ValueRef,
        lo: ValueRef,
        ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder =
            self.builder_mut().create::<crate::ops::Join, (ValueRef, ValueRef, Type)>(span);
        let op = op_builder(hi, lo, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Split `n` into limbs of type `limb_ty`, ordered from most-significant to least-significant.
    fn split(
        &mut self,
        n: ValueRef,
        limb_ty: Type,
        span: SourceSpan,
    ) -> Result<(ValueRef, ValueRef), Report> {
        let op_builder = self.builder_mut().create::<crate::ops::Split, _>(span);
        let op = op_builder(n, limb_ty)?;
        let op = op.borrow();
        let lo = op.result_low().as_value_ref();
        let hi = op.result_high().as_value_ref();
        Ok((hi, lo))
    }

    #[allow(clippy::wrong_self_convention)]
    fn is_odd(&mut self, value: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::IsOdd, _>(span);
        let op = op_builder(value)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

impl<'f, B: ?Sized + Builder> ArithOpBuilder<'f, B> for FunctionBuilder<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        FunctionBuilder::builder(self)
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        FunctionBuilder::builder_mut(self)
    }
}

impl<'f> ArithOpBuilder<'f, OpBuilder> for &'f mut OpBuilder {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self
    }
}

impl<B: ?Sized + Builder> ArithOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
