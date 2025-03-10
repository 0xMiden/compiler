use crate::{
    dialects::{builtin::FunctionBuilder, test::*},
    Builder, BuilderExt, OpBuilder, Report, UnsafeIntrusiveEntityRef, ValueRef,
};

pub trait TestOpBuilder<'f, B: ?Sized + Builder> {
    fn u32(&mut self, value: u32, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Constant, _>(span);
        let constant = op_builder(Immediate::U32(value))?;
        Ok(constant.borrow().result().as_value_ref())
    }

    /// Two's complement addition which traps on overflow
    fn add(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Add, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement addition. Behavior is undefined if the result overflows.
    fn add_unchecked(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Add, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement addition which wraps around on overflow, e.g. `wrapping_add`
    fn add_wrapping(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Add, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement multiplication which traps on overflow
    fn mul(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Mul, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Checked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Unchecked two's complement multiplication. Behavior is undefined if the result overflows.
    fn mul_unchecked(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Mul, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Unchecked)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Two's complement multiplication which wraps around on overflow, e.g. `wrapping_mul`
    fn mul_wrapping(
        &mut self,
        lhs: ValueRef,
        rhs: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Mul, _>(span);
        let op = op_builder(lhs, rhs, crate::Overflow::Wrapping)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn shl(&mut self, lhs: ValueRef, rhs: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Shl, _>(span);
        let op = op_builder(lhs, rhs)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Loads a value of the type pointed to by the given pointer, on to the stack
    ///
    /// NOTE: This function will panic if `ptr` is not a pointer typed value
    fn load(&mut self, addr: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Load, _>(span);
        let op = op_builder(addr)?;
        Ok(op.borrow().result().as_value_ref())
    }

    /// Stores `value` to the address given by `ptr`
    ///
    /// NOTE: This function will panic if the pointer and pointee types do not match
    fn store(
        &mut self,
        ptr: ValueRef,
        value: ValueRef,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Store>, Report> {
        let op_builder = self.builder_mut().create::<Store, _>(span);
        op_builder(ptr, value)
    }

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

impl<'f, B: ?Sized + Builder> TestOpBuilder<'f, B> for FunctionBuilder<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        FunctionBuilder::builder(self)
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        FunctionBuilder::builder_mut(self)
    }
}

impl<'f> TestOpBuilder<'f, OpBuilder> for &'f mut OpBuilder {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self
    }
}

impl<B: ?Sized + Builder> TestOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
