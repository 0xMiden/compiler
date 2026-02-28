use alloc::{boxed::Box, rc::Rc};

use crate::{
    BlockRef, Builder, Context, Ident, OpBuilder, Type,
    diagnostics::Report,
    dialects::builtin::{
        BuiltinOpBuilder, Function, FunctionBuilder, FunctionRef, ModuleRef,
        attributes::{Signature, Visibility},
    },
    interner,
    pass::{OperationPass, Pass, PassManager},
};

/// Enable compiler-internal tracing and instrumentation during tests
#[cfg(feature = "logging")]
pub fn enable_compiler_instrumentation() {
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();
}

/// Enable compiler-internal tracing and instrumentation during tests
#[cfg(not(feature = "logging"))]
pub fn enable_compiler_instrumentation() {}

/// A [Test] sets up the common boilerplate for IR tests throughout the compiler that follow one of
/// a few typical patterns:
///
/// 1. The most common scenario is we want to define a function for the test and then work with
///    that function. In this scenario, creating the test and defining the function typically happen
///    via [`Test::new`].
/// 2. A variation of the previous scenario where we want to define a module with a primary function
///    that interacts with other symbols in the same module. In this scenario, the test is initially
///    created without any function, a module is defined, and then the main and secondary functions
///    defined at that point, e.g. `Test::default().in_module("bar").with_function("foo", ...)`
/// 3. We don't need a function, but some other operation type, which still benefits from having
///    the common boilerplate abstracted away.
///
/// # Features:
///
/// * Tests are automatically instrumented
/// * Tests are named to make diagnostics clearer
/// * Conveniences for applying passes to the primary test function.
pub struct Test {
    context: Rc<Context>,
    name: &'static str,
    builder: OpBuilder,
    module: Option<ModuleRef>,
    function: Option<FunctionRef>,
}

impl Default for Test {
    fn default() -> Self {
        enable_compiler_instrumentation();

        let context = Rc::new(Context::default());
        let builder = OpBuilder::new(context.clone());

        Self {
            context,
            name: "test",
            builder,
            module: None,
            function: None,
        }
    }
}

impl Test {
    /// Create a new test in a function named `name` with the given parameter and result types.
    pub fn new(name: &'static str, params: &[Type], results: &[Type]) -> Self {
        let mut test = Self::named(name);

        test.with_function(name, params, results);

        test
    }

    /// Create a new, empty test named `name`.
    ///
    /// The resulting test has no module or function associated with it, so you must either modify
    /// the test after creating it with this, or get the underlying [OpBuilder] and use that.
    pub fn named(name: &'static str) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    /// Modifies the test being built by creating a module named `name` and setting the builder
    /// insertion point to the end of the module body.
    ///
    /// NOTE: If this is called after `Test::new` or `Test::with_function` have been called, those
    /// functions are _not_ automatically moved into the module that is created.
    pub fn in_module(mut self, name: impl Into<interner::Symbol>) -> Self {
        let name = name.into();
        let module = self.builder.create_module(Ident::with_empty_span(name)).unwrap();
        // Ensure module body is present
        let module_body = module.borrow().body().as_region_ref();
        self.builder.create_block(module_body, None, &[]);
        self.module = Some(module);
        self
    }

    /// Modifies this test with a new primary function called `name` with the given parameter and
    /// result types.
    ///
    /// NOTE: If this is called after `Test::new`, or a previous call to `with_function`, then the
    /// previous function is _not_ erased or modified in any way - that is left up to the caller.
    pub fn with_function(&mut self, name: &'static str, params: &[Type], results: &[Type]) {
        self.name = name;
        let function = self
            .builder
            .create_function(
                Ident::with_empty_span(name.into()),
                Visibility::Public,
                Signature::new(&self.context, params.iter().cloned(), results.iter().cloned()),
            )
            .expect("failed to create function");

        // Initialize the function body
        let _ = FunctionBuilder::new(function, &mut self.builder);

        self.function = Some(function);
    }

    /// Defines a new secondary function for this test.
    ///
    /// This requires the test to have been constructed with a module using [`Test::in_module`],
    /// and will assert that this is the case.
    ///
    /// The function which is defined has public visibility, and uses the default calling convention.
    pub fn define_function(
        &mut self,
        name: impl Into<interner::Symbol>,
        params: &[Type],
        results: &[Type],
    ) -> FunctionRef {
        let module = self.module.expect("cannot define non-test functions without a module");
        let module_body = { module.borrow().body().entry_block_ref().unwrap() };
        self.builder.set_insertion_point_to_end(module_body);
        self.builder
            .create_function(
                Ident::with_empty_span(name.into()),
                Visibility::Public,
                Signature::new(&self.context, params.iter().cloned(), results.iter().cloned()),
            )
            .expect("failed to define function")
    }

    /// Get the name of the test and its primary function
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Get a reference to the current [Context]
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Get a reference-counted pointer to the current [Context]
    pub fn context_rc(&self) -> Rc<Context> {
        self.context.clone()
    }

    /// Get a [FunctionRef] corresponding to this tests' primary function.
    ///
    /// This will panic if the test was created without a function.
    pub fn function(&self) -> FunctionRef {
        self.function.unwrap()
    }

    /// Get a [ModuleRef] corresponding to the module this test is working within.
    ///
    /// This will panic if the test was created without a module.
    pub fn module(&self) -> ModuleRef {
        self.module.unwrap()
    }

    /// Get a [FunctionBuilder] for this tests' primary function.
    ///
    /// This will panic if the test was created without a function.
    pub fn function_builder(&mut self) -> FunctionBuilder<'_, OpBuilder> {
        FunctionBuilder::new(self.function(), &mut self.builder)
    }

    /// Get a mutable reference to the [OpBuilder] for this test
    pub fn builder_mut(&mut self) -> &mut OpBuilder {
        &mut self.builder
    }

    /// Get a [BlockRef] for the entry block of this test's primary function.
    ///
    /// This will panic if the test was created without a function.
    pub fn entry_block(&self) -> BlockRef {
        self.function.unwrap().borrow().entry_block()
    }

    /// Sets up a pass manager with a default instance of a pass `P`, and runs it over the primary
    /// function of this test.
    ///
    /// If `verify` is true, then the verifier is run on the resulting IR.
    pub fn apply_pass<P: Pass + Default>(&self, verify: bool) -> Result<(), Report> {
        let mut pm = PassManager::on::<Function>(self.context_rc(), crate::pass::Nesting::Implicit);
        pm.add_pass(Box::new(P::default()));
        pm.enable_verifier(verify);
        pm.run(self.function().as_operation_ref())
    }

    /// Sets up a pass manager with `pass`, and runs it over the primary function of this test.
    ///
    /// If `verify` is true, then the verifier is run on the resulting IR.
    pub fn apply_boxed_pass(
        &self,
        pass: Box<dyn OperationPass>,
        verify: bool,
    ) -> Result<(), Report> {
        let mut pm = PassManager::on::<Function>(self.context_rc(), crate::pass::Nesting::Implicit);
        pm.add_pass(pass);
        pm.enable_verifier(verify);
        pm.run(self.function().as_operation_ref())
    }

    /// Sets up a pass manager with `passes`, and runs it over the primary function of this test.
    ///
    /// If `verify` is true, then the verifier is run on the resulting IR.
    pub fn apply_passes(
        &mut self,
        passes: impl IntoIterator<Item = Box<dyn OperationPass>>,
        verify: bool,
    ) -> Result<(), Report> {
        let mut pm = PassManager::on::<Function>(self.context_rc(), crate::pass::Nesting::Implicit);
        for pass in passes {
            pm.add_pass(pass);
        }
        pm.enable_verifier(verify);
        pm.run(self.function().as_operation_ref())
    }
}
