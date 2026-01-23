use alloc::vec::Vec;

extern crate std;

// ================================ Build ======================================

/// Runs `cargo miden build` to build the .masp Package and returns the path
/// where it is stored.
pub fn build_package() -> std::path::PathBuf {
    let build_cmd = cargo_miden::BuildCommand { args: Vec::new() };

    let output = build_cmd.exec(cargo_miden::OutputType::Masm).expect("failed to build project.");

    let build_output = output.expect("failed to obtain build output.").unwrap_build_output();

    build_output.into_artifact_path()
}

// ============================= Test function ================================

/// Struct that represents a function marked with #[miden_test].
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

// =========================== Querying functions ==============================

/// Access all functions tagged with #[miden_test].
///
/// NOTE: currently we don't use `inventory`'s vector to execute tests, since we
/// rely on cargo's default registration mechanism. This stems from the fact
/// that we rely on the #[test] attribute for execution, since it enables
/// specific test execution from IDEs, like VSCode.  Using #[test], however,
/// generates the libtest related code, *even when libtest harness is off*. This
/// means that if both `inventory` and `#[test]` are used, every test gets run
/// run twice, once in [libtest_mimic::run] and another time in rust's libtest
/// harness.
///
/// Currently, this list is not used.
pub fn registered_test_function() -> impl Iterator<Item = &'static MidenTest> {
    inventory::iter::<MidenTest>.into_iter()
}
