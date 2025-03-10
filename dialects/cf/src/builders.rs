use midenc_hir2::{
    dialects::builtin::FunctionBuilder, BlockRef, Builder, BuilderExt, OpBuilder, Report,
    SourceSpan, UnsafeIntrusiveEntityRef, ValueRef,
};

use crate::*;

pub trait ControlFlowOpBuilder<'f, B: ?Sized + Builder> {
    fn br<A>(
        &mut self,
        block: BlockRef,
        args: A,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Br>, Report>
    where
        A: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<crate::ops::Br, (_, A)>(span);
        op_builder(block, args)
    }

    fn cond_br<T, F>(
        &mut self,
        cond: ValueRef,
        then_dest: BlockRef,
        then_args: T,
        else_dest: BlockRef,
        else_args: F,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::CondBr>, Report>
    where
        T: IntoIterator<Item = ValueRef>,
        F: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<crate::ops::CondBr, (_, _, T, _, F)>(span);
        op_builder(cond, then_dest, then_args, else_dest, else_args)
    }

    fn switch<TCases, TFallbackArgs>(
        &mut self,
        selector: ValueRef,
        cases: TCases,
        fallback: BlockRef,
        fallback_args: TFallbackArgs,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<crate::ops::Switch>, Report>
    where
        TCases: IntoIterator<Item = SwitchCase>,
        TFallbackArgs: IntoIterator<Item = ::midenc_hir2::ValueRef>,
    {
        let op_builder = self
            .builder_mut()
            .create::<crate::ops::Switch, (_, _, TFallbackArgs, TCases)>(span);
        op_builder(selector, fallback, fallback_args, cases)
    }

    fn select(
        &mut self,
        cond: ValueRef,
        a: ValueRef,
        b: ValueRef,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<Select, _>(span);
        let op = op_builder(cond, a, b)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

impl<'f, B: ?Sized + Builder> ControlFlowOpBuilder<'f, B> for FunctionBuilder<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        FunctionBuilder::builder(self)
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        FunctionBuilder::builder_mut(self)
    }
}

impl<'f> ControlFlowOpBuilder<'f, OpBuilder> for &'f mut OpBuilder {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self
    }
}

impl<B: ?Sized + Builder> ControlFlowOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
