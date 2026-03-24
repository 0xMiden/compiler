use midenc_hir::{
    Builder, BuilderExt, OpBuilder, Report, SourceSpan, Type, ValueRef,
    dialects::builtin::FunctionBuilder,
};

pub trait WasmOpBuilder<'f, B: ?Sized + Builder> {
    fn sign_extend(
        &mut self,
        arg: ValueRef,
        src_ty: Type,
        dst_ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::SignExtend, _>(span);
        let op = op_builder(arg, src_ty, dst_ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn i32_load8_s(&mut self, addr: ValueRef, span: SourceSpan) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<crate::ops::I32Load8S, _>(span);
        let op = op_builder(addr)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

impl<'f, B: ?Sized + Builder> WasmOpBuilder<'f, B> for FunctionBuilder<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        FunctionBuilder::builder(self)
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        FunctionBuilder::builder_mut(self)
    }
}

impl<'f> WasmOpBuilder<'f, OpBuilder> for &'f mut OpBuilder {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self
    }
}

impl<B: ?Sized + Builder> WasmOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
