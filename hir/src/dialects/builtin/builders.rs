mod component;
mod function;
mod module;
mod world;

pub use self::{component::*, function::*, module::*, world::*};
use super::ops::*;
use crate::{
    constants::ConstantData, Builder, BuilderExt, Ident, Immediate, OpBuilder, Report, Signature,
    SourceSpan, Spanned, Type, UnsafeIntrusiveEntityRef, ValueRef, Visibility,
};

pub trait BuiltinOpBuilder<'f, B: ?Sized + Builder> {
    fn create_interface(&mut self, name: Ident) -> Result<InterfaceRef, Report> {
        let op_builder = self.builder_mut().create::<Interface, (_,)>(name.span());
        op_builder(name)
    }

    fn create_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        let op_builder = self.builder_mut().create::<Module, (_,)>(name.span());
        op_builder(name)
    }

    fn create_function(
        &mut self,
        name: Ident,
        signature: Signature,
    ) -> Result<FunctionRef, Report> {
        let op_builder = self.builder_mut().create::<Function, (_, _)>(name.span());
        op_builder(name, signature)
    }

    fn create_global_variable(
        &mut self,
        name: Ident,
        visibility: Visibility,
        ty: Type,
    ) -> Result<GlobalVariableRef, Report> {
        let op_builder = self.builder_mut().create::<GlobalVariable, (_, _, _)>(name.span());
        op_builder(name, visibility, ty)
    }

    fn create_data_segment(
        &mut self,
        offset: u32,
        data: impl Into<ConstantData>,
        readonly: bool,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Segment>, Report> {
        let data = self.builder().context().create_constant(data);
        let op_builder = self.builder_mut().create::<Segment, (_, _, _)>(span);
        op_builder(offset, data, readonly)
    }

    fn unrealized_conversion_cast(
        &mut self,
        value: ValueRef,
        ty: Type,
        span: SourceSpan,
    ) -> Result<ValueRef, Report> {
        let op_builder = self.builder_mut().create::<UnrealizedConversionCast, (_, _)>(span);
        let op = op_builder(value, ty)?;
        Ok(op.borrow().result().as_value_ref())
    }

    fn ret<I>(
        &mut self,
        returning: I,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Ret>, Report>
    where
        I: IntoIterator<Item = ValueRef>,
    {
        let op_builder = self.builder_mut().create::<Ret, (I,)>(span);
        op_builder(returning)
    }

    fn ret_imm(
        &mut self,
        arg: Immediate,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<RetImm>, Report> {
        let op_builder = self.builder_mut().create::<RetImm, _>(span);
        op_builder(arg)
    }

    fn builder(&self) -> &B;
    fn builder_mut(&mut self) -> &mut B;
}

impl<'f, B: ?Sized + Builder> BuiltinOpBuilder<'f, B> for FunctionBuilder<'f, B> {
    #[inline(always)]
    fn builder(&self) -> &B {
        FunctionBuilder::builder(self)
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        FunctionBuilder::builder_mut(self)
    }
}

impl<'f> BuiltinOpBuilder<'f, OpBuilder> for &'f mut OpBuilder {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self
    }
}

impl<B: ?Sized + Builder> BuiltinOpBuilder<'_, B> for B {
    #[inline(always)]
    fn builder(&self) -> &B {
        self
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut B {
        self
    }
}
