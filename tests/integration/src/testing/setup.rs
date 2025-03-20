//! This module provides core utilities for setting up the necessary artifacts and state for tests.
use std::{path::PathBuf, rc::Rc};

use midenc_compile::LinkOutput;
use midenc_hir::{
    dialects::builtin::{
        self, ComponentBuilder, FunctionBuilder, FunctionRef, ModuleBuilder, WorldBuilder,
    },
    version::Version,
    BuilderExt, Context, Ident, Op, OpBuilder, Signature, SourceSpan,
};
use midenc_session::{InputFile, Session};

use super::format_report;

/// Enable compiler-internal tracing and instrumentation during tests
pub fn enable_compiler_instrumentation() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();
}

/// Create a valid [Context] representing a compiler session with a "dummy" input that doesn't
/// actually exist.
///
/// This is used to fulfill the requirement of having a valid [Context], when it won't actually be
/// used for compilation (at least, not via the main compiler entrypoint).
pub fn dummy_context(flags: &[&str]) -> Rc<Context> {
    let session = dummy_session(flags);
    let context = Rc::new(Context::new(session));
    midenc_codegen_masm::register_dialect_hooks(&context);
    midenc_hir_eval::register_dialect_hooks(&context);
    context
}

/// Create a valid [Session] with a "dummy" input that doesn't actually exist.
///
/// This is used when you need to call into some code that requires a valid [Session], but it won't
/// actually be used for compilation (or at least, not via the main compiler entrypoint).
pub fn dummy_session(flags: &[&str]) -> Rc<Session> {
    let dummy = InputFile::from_path(PathBuf::from("dummy.wasm")).unwrap();
    default_session([dummy], flags)
}

/// Create a valid [Context] for `inputs` with `argv`, with useful defaults.
pub fn default_context<S, I>(inputs: I, argv: &[S]) -> Rc<Context>
where
    I: IntoIterator<Item = InputFile>,
    S: AsRef<str>,
{
    let session = default_session(inputs, argv);
    let context = Rc::new(Context::new(session));
    midenc_codegen_masm::register_dialect_hooks(&context);
    midenc_hir_eval::register_dialect_hooks(&context);
    context
}

/// Create a valid [Session] for compiling `inputs` with `argv`, with useful defaults.
pub fn default_session<S, I>(inputs: I, argv: &[S]) -> Rc<Session>
where
    I: IntoIterator<Item = InputFile>,
    S: AsRef<str>,
{
    use midenc_session::diagnostics::reporting::{self, ReportHandlerOpts};

    let result = reporting::set_hook(Box::new(|_| {
        let wrapping_width = 300; // avoid wrapped file paths in the backtrace
        Box::new(ReportHandlerOpts::new().width(wrapping_width).build())
    }));
    if result.is_ok() {
        reporting::set_panic_hook();
    }

    let argv = argv.iter().map(|arg| arg.as_ref());
    let session = midenc_compile::Compiler::new_session(inputs, None, argv);
    Rc::new(session)
}

/// Create a [LinkOutput] representing an empty component named `root:root@1.0.0`.
///
/// Callers may then populate the world/component as they see fit for a particular test.
pub fn build_empty_component_for_test(context: Rc<Context>) -> LinkOutput {
    let mut builder = OpBuilder::new(context.clone());
    let world = {
        let builder = builder.create::<builtin::World, ()>(SourceSpan::default());
        builder().unwrap_or_else(|err| panic!("failed to create world:\n{}", format_report(err)))
    };
    let mut world_builder = WorldBuilder::new(world);
    let name = Ident::with_empty_span("root".into());
    let ns_name = Ident::with_empty_span("root_ns".into());
    let component = world_builder
        .define_component(ns_name, name, Version::new(1, 0, 0))
        .unwrap_or_else(|err| panic!("failed to define component:\n{}", format_report(err)));

    let mut link_output = LinkOutput {
        world,
        component,
        masm: Default::default(),
        mast: Default::default(),
        packages: Default::default(),
        account_component_metadata_bytes: None,
    };
    link_output
        .link_libraries_from(context.session())
        .unwrap_or_else(|err| panic!("{}", format_report(err)));
    link_output
}

/// Defines a module called `test` in `component`, containing a function called `main` with
/// `signature`, designed to be invoked as the entrypoint of a test case.
///
/// The body of `main` is populated by `build`, which must ensure that it returns from `main`
/// with the results expected by `signature`.
///
/// A reference to the generated `main` function is returned, should the caller wish to modify it
/// further.
pub fn build_entrypoint<F>(
    component: builtin::ComponentRef,
    signature: &Signature,
    build: F,
) -> FunctionRef
where
    F: Fn(&mut FunctionBuilder<'_, OpBuilder>),
{
    let module = {
        let mut component_builder = ComponentBuilder::new(component);
        component_builder
            .define_module(Ident::with_empty_span("test".into()))
            .unwrap_or_else(|err| panic!("failed to define module:\n{}", format_report(err)))
    };
    let mut function = {
        let mut module_builder = ModuleBuilder::new(module);
        module_builder
            .define_function(Ident::with_empty_span("main".into()), signature.clone())
            .unwrap_or_else(|err| panic!("failed to define function:\n{}", format_report(err)))
    };

    // Define function body
    {
        let mut func = function.borrow_mut();
        let context = func.as_operation().context_rc();
        let mut builder = OpBuilder::new(context);
        let mut builder = FunctionBuilder::new(&mut func, &mut builder);
        build(&mut builder);
    }

    println!("# Entrypoint\n{}\n", function.borrow().as_operation());

    function
}
