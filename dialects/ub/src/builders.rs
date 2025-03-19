use midenc_hir::{
    dialects::builtin::FunctionBuilder, Builder, BuilderExt, OpBuilder, SourceSpan, Type,
    UnsafeIntrusiveEntityRef, ValueRef,
};

use crate::*;

pub trait UndefinedBehaviorOpBuilder<'f, B: ?Sized + Builder> {
    fn poison(&mut self, ty: Type, span: SourceSpan) -> ValueRef {
        let op_builder = self.builder_mut().create::<Poison, _>(span);
        let op = op_builder(PoisonAttr::new(ty)).expect("invalid poison attribute");
        op.borrow().result().as_value_ref()
    }

    fn unreachable(&mut self, span: SourceSpan) -> UnsafeIntrusiveEntityRef<Unreachable> {
        let op_builder = self.builder_mut().create::<Unreachable, _>(span);
        op_builder().unwrap()
    }

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

impl<'f, B: ?Sized + Builder> UndefinedBehaviorOpBuilder<'f, B> for FunctionBuilder<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        FunctionBuilder::builder(self)
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        FunctionBuilder::builder_mut(self)
    }
}

impl<'f> UndefinedBehaviorOpBuilder<'f, OpBuilder> for &'f mut OpBuilder {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self
    }
}

impl<B: ?Sized + Builder> UndefinedBehaviorOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
