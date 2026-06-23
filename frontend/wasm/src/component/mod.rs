//! Support for the Wasm component model translation
//!
//! This module contains all of the internal type definitions to parse and
//! translate the component model.

pub(crate) mod build_ir;
mod canon_abi_utils;
mod flat;
mod lift_exports;
pub(crate) mod lower_imports;
mod parser;
mod shim_bypass;
mod translator;
mod types;

pub use self::{parser::*, types::*};

#[cfg(test)]
pub(super) mod test_support {
    //! Test helpers for synthetic component CanonABI wrapper fixtures.

    use alloc::{rc::Rc, sync::Arc};
    use core::cell::RefCell;

    use midenc_dialect_cf::{self as cf, ControlFlowOpBuilder};
    use midenc_dialect_ub as ub;
    use midenc_hir::{
        BuilderExt, CallConv, Context, EnumType, FunctionType, Ident, Op, Operation, SourceSpan,
        StructType, SymbolName, SymbolTable, Type, ValueRef, Variant, Visibility, WalkResult,
        dialects::builtin::{
            BuiltinOpBuilder, ComponentBuilder, Function, FunctionRef, ModuleBuilder, World,
            WorldBuilder, attributes::Signature,
        },
        version::Version,
    };

    use crate::module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    };

    /// Builds a HIR type for a two-case unit-only component variant.
    pub fn unit_only_variant_type() -> Type {
        Type::Enum(Arc::new(
            EnumType::new(
                "unit-only".into(),
                Type::U8,
                [
                    Variant::c_like("first".into(), Some(0)),
                    Variant::c_like("second".into(), Some(1)),
                ],
            )
            .expect("unit-only enum should be valid"),
        ))
    }

    /// Builds a HIR type for a two-case component variant with scalar payloads.
    pub fn scalar_payload_variant_type() -> Type {
        Type::Enum(Arc::new(
            EnumType::new(
                "scalar-payload".into(),
                Type::U8,
                [
                    Variant::new("first".into(), Type::I32, Some(0)),
                    Variant::new("second".into(), Type::I32, Some(1)),
                ],
            )
            .expect("scalar-payload enum should be valid"),
        ))
    }

    /// Builds a HIR type for a two-case variant with mixed 32/64-bit payloads.
    ///
    /// The payload lanes join `i32` and `u64` into one widened `i64` slot, exercising the
    /// positional payload-join rules.
    pub fn mixed_payload_variant_type() -> Type {
        Type::Enum(Arc::new(
            EnumType::new(
                "mixed-payload".into(),
                Type::U8,
                [
                    Variant::new("first".into(), Type::I32, Some(0)),
                    Variant::new("second".into(), Type::U64, Some(1)),
                ],
            )
            .expect("mixed-payload enum should be valid"),
        ))
    }

    /// Builds a HIR type for an option-shaped variant with a felt payload.
    pub fn option_of_felt_type() -> Type {
        Type::Enum(Arc::new(
            EnumType::new(
                "option-felt".into(),
                Type::U8,
                [
                    Variant::c_like("none".into(), Some(0)),
                    Variant::new("some".into(), Type::Felt, Some(1)),
                ],
            )
            .expect("option-shaped enum should be valid"),
        ))
    }

    /// Builds a HIR type for a record with a scalar and a payload-variant field.
    pub fn record_with_variant_field_type() -> Type {
        let variant_field = scalar_payload_variant_type();
        Type::from(StructType::new(vec![Type::U32, variant_field]))
    }

    /// Returns HIR type fixtures whose type-only Canonical ABI helpers must agree with flattening.
    ///
    /// The fixtures cover the supported lowering domain: scalars, records, unit-only variants,
    /// payload variants (including joined mixed-width lanes), and nesting.
    pub fn canonical_agreement_fixtures() -> Vec<Type> {
        let scalars = [
            Type::I1,
            Type::I8,
            Type::U8,
            Type::I16,
            Type::U16,
            Type::I32,
            Type::U32,
            Type::I64,
            Type::U64,
            Type::Felt,
        ];

        scalars
            .into_iter()
            .chain([
                two_field_record_type(),
                unit_only_variant_type(),
                scalar_payload_variant_type(),
                mixed_payload_variant_type(),
                option_of_felt_type(),
                record_with_variant_field_type(),
            ])
            .collect()
    }

    /// Builds a HIR type for a two-field record result.
    pub fn two_field_record_type() -> Type {
        Type::from(StructType::new(vec![Type::U32, Type::U32]))
    }

    /// Creates an empty world fixture for component CanonABI tests.
    pub fn test_world() -> (Rc<Context>, WorldBuilder) {
        let context = Rc::new(Context::default());
        let mut builder = midenc_hir::OpBuilder::new(context.clone());
        let world =
            builder.create::<World, ()>(SourceSpan::default())().expect("failed to create world");
        (context, WorldBuilder::new(world))
    }

    /// Creates a world fixture with a declared "core" module for import lowering tests.
    pub fn world_with_core_module() -> (Rc<Context>, WorldBuilder, ModuleBuilder) {
        let (context, mut world_builder) = test_world();
        let module = world_builder
            .declare_module("core".into())
            .expect("failed to declare core module");
        (context, world_builder, ModuleBuilder::new(module))
    }

    /// Creates a world fixture with a "miden:test" component and its "core" module for export
    /// lifting tests.
    pub fn component_with_core_module() -> (Rc<Context>, ComponentBuilder, ModuleBuilder) {
        let (context, mut world_builder) = test_world();
        let component = world_builder
            .define_component("miden".into(), "test".into(), Version::new(1, 0, 0))
            .expect("failed to define component");
        let mut component_builder = ComponentBuilder::new(component);
        let core_module = component_builder
            .define_module(Ident::with_empty_span("core".into()))
            .expect("failed to define core module");
        let module_builder = ModuleBuilder::new(core_module);
        (context, component_builder, module_builder)
    }

    /// Builds a single-function module fixture with `params` and runs `build` in its entry block.
    ///
    /// Returns the context together with the function: the context owns the IR arena, so it must
    /// stay alive while the returned function is inspected.
    pub fn build_module_function(
        name: &'static str,
        params: Vec<Type>,
        build: impl FnOnce(
            &mut FunctionBuilderExt<'_, midenc_hir::OpBuilder<SSABuilderListener>>,
            &[ValueRef],
        ),
    ) -> (Rc<Context>, FunctionRef) {
        let (context, _world_builder, mut module_builder) = world_with_core_module();
        let signature =
            Signature::new(&context, FunctionType::new(CallConv::Fast, params, vec![]).params, []);
        let function = module_builder
            .define_function(Ident::with_empty_span(name.into()), Visibility::Public, signature)
            .expect("failed to define function");

        {
            let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
            let mut op_builder = midenc_hir::OpBuilder::new(context.clone())
                .with_listener(SSABuilderListener::new(func_ctx));
            let mut fb = FunctionBuilderExt::new(function, &mut op_builder);
            let entry_block = fb.current_block();
            fb.seal_block(entry_block);
            let args: Vec<ValueRef> = entry_block
                .borrow()
                .arguments()
                .iter()
                .copied()
                .map(|arg| arg as ValueRef)
                .collect();

            build(&mut fb, &args);

            let exit_block = fb.create_block();
            fb.br(exit_block, [], SourceSpan::default()).expect("failed to branch to exit");
            fb.seal_block(exit_block);
            fb.switch_to_block(exit_block);
            fb.ret([], SourceSpan::default()).expect("failed to return");
        }

        (context, function)
    }

    /// Counts operations matching `predicate` within `function`.
    pub fn count_ops(function: FunctionRef, predicate: impl Fn(&Operation) -> bool) -> usize {
        let mut count = 0;
        function
            .borrow()
            .as_operation()
            .prewalk(|op: &Operation| {
                if predicate(op) {
                    count += 1;
                }
                WalkResult::<()>::Continue(())
            })
            .into_result()
            .expect("operation walk should not fail");

        count
    }

    /// Counts generated variant-validation control-flow operations in `function`.
    pub fn count_validation_ops(function: FunctionRef) -> (usize, usize) {
        (
            count_ops(function, |op| op.is::<cf::Switch>()),
            count_ops(function, |op| op.is::<ub::Unreachable>()),
        )
    }

    /// Looks up a generated component function by name.
    pub fn component_function(component_builder: &ComponentBuilder, name: &str) -> FunctionRef {
        let symbol = SymbolName::intern(name);
        let symbol_ref = component_builder
            .component
            .borrow()
            .get(symbol)
            .expect("expected component function");
        let op = symbol_ref.borrow();
        op.as_symbol_operation()
            .downcast_ref::<Function>()
            .expect("expected symbol to be a function")
            .as_function_ref()
    }
}
