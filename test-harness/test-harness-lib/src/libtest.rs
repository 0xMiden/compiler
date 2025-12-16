use alloc::vec::Vec;

// ============================= Test function ================================

/// Struct that represents a function marked with #[miden-test].
/// NOTE: This structure is only intended to be used by the
/// miden-test-harness-macros crate.
pub struct MidenTest {
    /// The name of the test, normally whatever text is followed by the `fn`
    /// keyword..
    pub name: &'static str,

    /// Actual test function.
    pub test_fn: fn() -> (),
}

// Register MidenTest as a pluging in order for it to be collected.
inventory::collect!(MidenTest);

pub use inventory::submit as miden_test_submit;

impl From<MidenTest> for libtest_mimic::Trial {
    fn from(value: MidenTest) -> Self {
        libtest_mimic::Trial::test(value.name, runner(value.test_fn))
    }
}

impl From<&MidenTest> for libtest_mimic::Trial {
    fn from(value: &MidenTest) -> Self {
        libtest_mimic::Trial::test(value.name, runner(value.test_fn))
    }
}

pub struct MidenTestArguments(libtest_mimic::Arguments);

impl From<MidenTestArguments> for libtest_mimic::Arguments {
    fn from(value: MidenTestArguments) -> Self {
        value.0
    }
}

// ============================= Test arguments ================================

impl MidenTestArguments {
    pub fn from_args() -> Self {
        let inner_args = libtest_mimic::Arguments::from_args();
        Self(inner_args)
    }
}

// Wrapper used to make normal rust function with libtest.
fn runner(test: fn() -> ()) -> impl FnOnce() -> Result<(), libtest_mimic::Failed> + Send + 'static {
    move || {
        test();
        Ok(())
    }
}

// ============================ Executing tests ===============================

pub fn run(args: MidenTestArguments) {
    let args = args.into();

    let tests: Vec<libtest_mimic::Trial> =
        inventory::iter::<MidenTest>.into_iter().map(|test| test.into()).collect();

    let conclusion = libtest_mimic::run(&args, tests);

    conclusion.exit()
}
