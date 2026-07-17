use std::rc::Rc;

use midenc_compile::{MidenComponent, Stage, stages::CodegenStage};
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    BuilderExt, Context, Ident, OpBuilder, SourceSpan, Visibility,
    dialects::builtin::{
        self, BuiltinOpBuilder, ComponentBuilder, FunctionBuilder, ModuleBuilder, WorldBuilder,
        attributes::Signature,
    },
    version::Version,
};

#[test]
fn codegen_stage_accepts_ops_legal_for_masm() {
    let context = Rc::new(Context::default());
    let component = build_test_component(context.clone(), |function_builder| {
        function_builder.ret(None, SourceSpan::UNKNOWN).unwrap();
    });

    if let Err(err) = CodegenStage.run(component, context) {
        panic!("codegen unexpectedly rejected legal MASM IR: {err}");
    }
}

#[test]
fn codegen_stage_fails_on_ops_not_legal_for_masm() {
    let context = Rc::new(Context::default());
    let component = build_test_component(context.clone(), |function_builder| {
        let _bytes = function_builder.bytes(&[1, 2, 3, 4], SourceSpan::UNKNOWN).unwrap();
        function_builder.ret(None, SourceSpan::UNKNOWN).unwrap();
    });

    let err = match CodegenStage.run(component, context) {
        Ok(_) => panic!("codegen unexpectedly accepted an unsupported HIR op"),
        Err(err) => err,
    };
    let message = format!("{err}");

    assert!(message.contains("hir.bytes"));
    assert!(message.contains("does not implement HirLowering"));
}

fn build_test_component(
    context: Rc<Context>,
    build: impl FnOnce(&mut FunctionBuilder<'_, OpBuilder>),
) -> MidenComponent {
    let mut builder = OpBuilder::new(context.clone());
    let world = builder.create::<builtin::World, ()>(SourceSpan::UNKNOWN)().unwrap();
    let mut world_builder = WorldBuilder::new(world);
    let component = world_builder
        .define_component(
            Ident::with_empty_span("test_ns".into()),
            Ident::with_empty_span("test".into()),
            Version::new(1, 0, 0),
        )
        .unwrap();

    let mut component_builder = ComponentBuilder::new(component);
    let module = component_builder.define_module(Ident::with_empty_span("test".into())).unwrap();
    let signature = Signature::new(&context, [], []);
    let mut module_builder = ModuleBuilder::new(module);
    let function = module_builder
        .define_function(Ident::with_empty_span("main".into()), Visibility::Public, signature)
        .unwrap();

    let mut builder = OpBuilder::new(context);
    let mut function_builder = FunctionBuilder::new(function, &mut builder);
    build(&mut function_builder);

    MidenComponent {
        world,
        component: Some(component),
        sections: Default::default(),
    }
}
