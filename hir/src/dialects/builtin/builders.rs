mod component;
mod function;
mod module;
mod world;

pub use self::{component::*, function::*, module::*, world::*};
use super::{attributes::Signature, ops::*};
use crate::{
    AsCallableSymbolRef, Builder, BuilderExt, Ident, Immediate, OpBuilder, OpRegistration, Report,
    SourceSpan, Spanned, SymbolName, SymbolTable, Type, UnsafeIntrusiveEntityRef, ValueRef,
    Visibility, constants::ConstantData,
};

pub trait BuiltinOpBuilder<'f, B: ?Sized + Builder> {
    /// Create an [Interface] named by the raw segment `name`, spelled as
    /// [Interface::canonical_name] does.
    ///
    /// Returns an error when Miden Assembly cannot spell `name`, or it contains `::`, or the
    /// interface cannot be defined where it is inserted.
    fn create_interface(&mut self, name: Ident) -> Result<InterfaceRef, Report> {
        let name = Ident::new(Interface::canonical_name(name.name)?, name.span);
        ensure_definable::<Interface, _>(self.builder(), name.name)?;
        let op_builder = self.builder_mut().create::<Interface, (_,)>(name.span());
        op_builder(name)
    }

    /// Create a [Module] named by the raw segment `name`, spelled as [Module::canonical_name]
    /// does.
    ///
    /// Returns an error when Miden Assembly cannot spell `name`, or it contains `::`, or the
    /// module cannot be defined where it is inserted.
    fn create_module(&mut self, name: Ident) -> Result<ModuleRef, Report> {
        let name = Ident::new(Module::canonical_name(name.name)?, name.span);
        ensure_definable::<Module, _>(self.builder(), name.name)?;
        let op_builder = self.builder_mut().create::<Module, (_,)>(name.span());
        op_builder(name)
    }

    /// Create a [Function] named `name`.
    ///
    /// Returns an error when the function cannot be defined where it is inserted.
    fn create_function(
        &mut self,
        name: Ident,
        visibility: Visibility,
        signature: Signature,
    ) -> Result<FunctionRef, Report> {
        ensure_definable::<Function, _>(self.builder(), name.name)?;
        let op_builder = self.builder_mut().create::<Function, (_, _, _)>(name.span());
        op_builder(name, visibility, signature)
    }

    /// Create a [FunctionAlias] referring to `target`.
    ///
    /// Returns an error when the alias cannot be defined where it is inserted.
    fn create_function_alias<C: AsCallableSymbolRef>(
        &mut self,
        name: Ident,
        visibility: Visibility,
        target: C,
    ) -> Result<FunctionAliasRef, Report> {
        ensure_definable::<FunctionAlias, _>(self.builder(), name.name)?;
        let op_builder = self.builder_mut().create::<FunctionAlias, (_, _, C)>(name.span());
        op_builder(name, visibility, target)
    }

    /// Create a [GlobalVariable] named `name`.
    ///
    /// Returns an error when the global cannot be defined where it is inserted.
    fn create_global_variable(
        &mut self,
        name: Ident,
        visibility: Visibility,
        ty: Type,
    ) -> Result<GlobalVariableRef, Report> {
        ensure_definable::<GlobalVariable, _>(self.builder(), name.name)?;
        let op_builder = self.builder_mut().create::<GlobalVariable, (_, _, _)>(name.span());
        op_builder(name, visibility, ty)
    }

    /// Create a [FunctionTable] with `num_slots` slots and no initialized entries.
    ///
    /// Returns an error when the table cannot be defined where it is inserted.
    fn create_function_table(
        &mut self,
        name: Ident,
        visibility: Visibility,
        num_slots: u32,
    ) -> Result<FunctionTableRef, Report> {
        ensure_definable::<FunctionTable, _>(self.builder(), name.name)?;
        let op_builder = self.builder_mut().create::<FunctionTable, (_, _, _)>(name.span());
        op_builder(name, visibility, num_slots)
    }

    fn create_data_segment(
        &mut self,
        offset: u32,
        data: impl Into<ConstantData>,
        readonly: bool,
        span: SourceSpan,
    ) -> Result<UnsafeIntrusiveEntityRef<Segment>, Report> {
        let id = self.builder().context().create_constant(data);
        let data = self.builder().context().get_constant(id);
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

/// Returns an error when a `T` named `name` cannot be defined in the symbol table `builder`
/// inserts into: a symbol there has that name or the same path (see
/// `<dyn SymbolTable>::can_define`), or the new symbol would be at or below the
/// path of a world-level component (see [World::reject_symbol_shadowed_by_component]).
///
/// A builder without a symbol table at its insertion point defines anything. Inserting a symbol
/// that conflicts would otherwise fail an assertion.
fn ensure_definable<T: OpRegistration, B: ?Sized + Builder>(
    builder: &B,
    name: SymbolName,
) -> Result<(), Report> {
    let Some(owner) = builder.insertion_block().and_then(|block| block.borrow().parent_op()) else {
        return Ok(());
    };
    // The table the op is registered in on insertion, see `Operation::nearest_symbol_table`
    let table = if owner.name().implements::<dyn SymbolTable>() {
        owner
    } else {
        let Some(table) = owner.borrow().nearest_symbol_table() else {
            return Ok(());
        };
        table
    };
    let kind = builder
        .context()
        .get_or_register_dialect::<T::Dialect>()
        .expect_registered_name::<T>();
    table
        .borrow()
        .as_trait::<dyn SymbolTable>()
        .expect("a symbol table")
        .can_define(name, &kind)
        .map_err(Report::msg)?;
    World::reject_symbol_shadowed_by_component(table, name, &kind)
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
