use std::{path::Path, rc::Rc};

use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};
use midenc_compile::{MidenComponent, pipeline::backend::codegen};
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    BuilderExt, Context, Ident, OpBuilder, SourceSpan, Visibility,
    dialects::builtin::{
        self, BuiltinOpBuilder, ComponentBuilder, FunctionBuilder, ModuleBuilder, WorldBuilder,
        attributes::Signature,
    },
    version::Version,
};

/// Code generation accepts a component built only from operations it can lower.
///
/// The two tests here are about [`codegen`]'s *legalization* step, which is the one phase that
/// can reject HIR outright — so they call the backend phase directly rather than driving a
/// compilation: there is no assembler, no project, and no route involved in the claim.
#[test]
fn codegen_accepts_ops_legal_for_masm() {
    let context = Rc::new(Context::default());
    let component = build_test_component(context.clone(), |function_builder| {
        function_builder.ret(None, SourceSpan::UNKNOWN).unwrap();
    });

    if let Err(err) = codegen(component, context) {
        panic!("codegen unexpectedly rejected legal MASM IR: {err}");
    }
}

/// And it rejects one built from an operation that has no lowering, naming the operation.
#[test]
fn codegen_fails_on_ops_not_legal_for_masm() {
    let context = Rc::new(Context::default());
    let component = build_test_component(context.clone(), |function_builder| {
        let _bytes = function_builder.bytes(&[1, 2, 3, 4], SourceSpan::UNKNOWN).unwrap();
        function_builder.ret(None, SourceSpan::UNKNOWN).unwrap();
    });

    let err = match codegen(component, context) {
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
        source_provenance: ProjectSourceProvenanceInputs {
            root: SourceFileProvenance {
                path: Path::new(file!()).to_path_buf().into_boxed_path(),
                content: String::new().into_boxed_str(),
            },
            support: Default::default(),
        },
    }
}
