use std::{path::Path, rc::Rc};

use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};
use midenc_compile::{
    MidenComponent,
    pipeline::backend::{apply_rewrites, codegen},
};
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    BuilderExt, Context, Ident, OpBuilder, SourceSpan, Visibility,
    diagnostics::Uri,
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

/// A world whose `hir.dyncall` needs seventeen operand stack elements at once: thirteen argument
/// field elements beside the four-element callee root.
///
/// Shaped like the `hir.dyncall` fixtures in `midenc-codegen-masm`, with the arguments reversed so
/// none of them is already in place. Thirteen is one past the budget MASM legalization enforces.
const WORLD_WITH_AN_OVERSIZED_DYNCALL: &str = r#"
builtin.world {
builtin.component private @"test_ns:test@1.0.0" {
    builtin.module public @test {
        builtin.function public extern("C") @dispatch(%r0: felt, %r1: felt, %r2: felt, %r3: felt, %a0: felt, %a1: felt, %a2: felt, %a3: felt, %a4: felt, %a5: felt, %a6: felt, %a7: felt, %a8: felt, %a9: felt, %a10: felt, %a11: felt, %a12: felt) -> felt {
            %result = hir.dyncall [%r0, %r1, %r2, %r3](%a12, %a11, %a10, %a9, %a8, %a7, %a6, %a5, %a4, %a3, %a2, %a1, %a0) : extern("component-model") (felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt, felt) -> (felt);
            builtin.ret %result : (felt);
        };
    };
};
};
"#;

/// The rewrites report an operation whose operands cannot all be reachable at once, rather than
/// panicking inside the spill analysis.
///
/// The bound itself belongs to MASM legalization, which runs in [`codegen`] — but that is *after*
/// the rewrites, and the spill analysis in [`apply_rewrites`] is what such an operation meets
/// first on the default pipeline. `.hir` text is the input that can carry one there: the Wasm
/// frontend rejects an over-budget stored procedure at translation, so no Rust source reaches
/// this. Like the tests above, this calls the backend phase directly, as there is no route,
/// project, or assembler in the claim.
#[test]
fn apply_rewrites_rejects_operands_that_cannot_fit_the_operand_stack() {
    let context = Rc::new(Context::default());
    let config = midenc_hir::parse::ParserConfig {
        context: context.clone(),
        verify: true,
    };
    let world = midenc_hir::parse::parse_any(
        config,
        Uri::new("oversized_dyncall.hir"),
        WORLD_WITH_AN_OVERSIZED_DYNCALL,
    )
    .expect("the fixture parses and verifies");

    let err = match apply_rewrites(world, context) {
        Ok(_) => panic!("the rewrites unexpectedly accepted a call that cannot be scheduled"),
        Err(err) => err,
    };
    let message = format!("{err}");

    assert!(message.contains("hir.dyncall"), "{message}");
    assert!(message.contains("needs 17 operand stack elements at once"), "{message}");
    assert!(message.contains("only 16 are addressable"), "{message}");
}
