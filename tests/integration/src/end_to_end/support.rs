use std::{marker::PhantomData, sync::Arc};

use miden_assembly::{
    Assembler, DefaultSourceManager,
    ast::{Module, ModuleKind, Path as MasmPath},
};
use miden_core::Felt;
use miden_core_lib::{CoreLibrary, handlers::u64_div::U64DivError};
use miden_mast_package::Package;
use miden_processor::{DefaultHost, ExecutionError, operation::OperationError};
use midenc_hir::diagnostics::PrintDiagnostic;
use num_traits::{PrimInt, Unsigned};
use proptest::{
    prelude::*,
    test_runner::{Config, TestError, TestRunner},
};

use crate::compiler_test::{sdk_alloc_crate_path, sdk_crate_path};

const INTRINSICS_ROOT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../codegen/masm/intrinsics/mod.masm");

/// Links the core library packages into the assembler.
fn link_core_packages(assembler: &mut Assembler, core_library: &CoreLibrary) {
    for package in core_library.packages() {
        let package_name = package.name.clone();
        assembler
            .link_package(package, miden_assembly::Linkage::Dynamic)
            .unwrap_or_else(|err| panic!("failed to link package '{package_name}': {err}"));
    }
}

/// Assembles an executable program that wraps `procedure_body` inside a procedure that is called
/// as entry point.
///
/// Both i32 and i64 intrinsics modules are statically linked so the body can call the intrinsics
/// of either type by their fully-qualified path (`::intrinsics::i32::*` or `::intrinsics::i64::*`).
pub(super) fn assemble_test_program(procedure_body: &str) -> Arc<Package> {
    let source_manager = Arc::new(DefaultSourceManager::default());
    let core_library = CoreLibrary::default();
    let mut assembler = Assembler::new(source_manager.clone());
    link_core_packages(&mut assembler, &core_library);

    // Parse the intrinsics
    assembler
        .compile_and_statically_link_from_root(INTRINSICS_ROOT, Some(MasmPath::new("intrinsics")))
        .unwrap_or_else(|err| panic!("{}", PrintDiagnostic::new(err)));

    // Parse the test module with its fully-qualified path
    let test_module_source = format!("pub proc test_intrinsic\n{procedure_body}\nend");
    let test_module = miden_assembly::ModuleParser::new(Some(ModuleKind::Library))
        .parse_str(Some(MasmPath::new("test")), test_module_source, source_manager.clone())
        .unwrap_or_else(|err| panic!("{}", PrintDiagnostic::new(err)));

    let library = assembler
        .assemble_library("test", test_module, None::<Box<Module>>)
        .unwrap_or_else(|err| panic!("{}", PrintDiagnostic::new(err)));

    let mut assembler = Assembler::new(source_manager);
    link_core_packages(&mut assembler, &core_library);
    assembler
        .with_package(library.into(), miden_assembly::Linkage::Static)
        .expect("failed to add library package as dependency")
        .assemble_program(
            "program",
            r#"
use miden::core::sys

begin
    exec.::test::test_intrinsic
    exec.sys::truncate_stack
end
"#,
        )
        .map(Arc::from)
        .unwrap_or_else(|err| panic!("{}", PrintDiagnostic::new(err)))
}

/// Returns a [`DefaultHost`] with the Miden core library loaded.
///
/// The core library registers the event handlers required to execute core helpers that rely on
/// the advice provider.
pub(crate) fn default_host_with_core_lib() -> DefaultHost {
    use miden_processor::HostLibrary;
    let core_library = CoreLibrary::default();
    let mut host = DefaultHost::default();
    host.load_library(HostLibrary::from(&core_library))
        .expect("failed to load core library into host");
    host
}

/// Describes the trap expected by the execution of an intrinsic.
///
/// Variants mirror [`OperationError`] variants that can be produced by i32 intrinsics.
#[derive(Debug, Clone)]
pub(super) enum TrapExpectation {
    /// Expect `FailedAssertion { err_code: 0, err_msg: None }`, produced by overflow traps.
    FailedAssertionOverflow,
    DivideByZero,
}

impl TrapExpectation {
    /// Returns `Ok(())` if `vm_err` matches the expectation.
    pub(super) fn check(&self, vm_err: &ExecutionError) -> Result<(), String> {
        match (self, vm_err) {
            (
                TrapExpectation::FailedAssertionOverflow,
                ExecutionError::OperationError {
                    err:
                        OperationError::FailedAssertion {
                            err_code,
                            err_msg: None,
                        },
                    ..
                },
            ) if *err_code == Felt::ZERO => Ok(()),
            (
                TrapExpectation::DivideByZero,
                ExecutionError::OperationError {
                    err: OperationError::DivideByZero,
                    ..
                },
            ) => Ok(()),
            // 64-bit int division is performed by the core library's `u64::div` procedure, which reports errors via the `U64_DIV` event handler.
            (TrapExpectation::DivideByZero, ExecutionError::EventError { error, .. })
                if error
                    .downcast_ref::<U64DivError>()
                    .is_some_and(|e| matches!(e, U64DivError::DivideByZero)) =>
            {
                Ok(())
            }
            _ => Err(format!("expected err {:?} but VM produced: {:?}", self, vm_err)),
        }
    }
}

pub(super) fn cargo_toml(name: &str) -> String {
    let sdk_alloc_path = sdk_alloc_crate_path();
    let sdk_path = sdk_crate_path();
    format!(
        r#"
                [package]
                name = "{name}"
                version = "0.0.1"
                edition = "2024"
                authors = []

                [lib]
                crate-type = ["cdylib"]

                [dependencies]
                miden-sdk-alloc = {{ path = "{sdk_alloc_path}" }}
                miden = {{ path = "{sdk_path}" }}

                [profile.release]
                # optimize the output for size
                opt-level = "z"
                panic = "abort"

                [profile.dev]
                panic = "abort"
                opt-level = 1
                debug-assertions = true
                overflow-checks = false
                debug = false

            "#,
        sdk_alloc_path = sdk_alloc_path.display(),
        sdk_path = sdk_path.display(),
    )
}

pub(super) fn miden_project_toml(name: &str) -> String {
    format!(
        r#"
                [package]
                name = "{name}"
                version = "0.0.1"

                [lib]
                # Core Wasm modules use the frontend's synthetic wrapper component identity.
                namespace = "root_ns:root@1.0.0"
                path = "src/lib.rs"

                [dependencies]
                miden-core = "*"
            "#,
    )
}

/// The number of randomly generated cases run after the enumerated edge cases.
///
/// The edge cases in a [`NumericCases`] run exhaustively, once each, so the random
/// tail only has to cover behavior away from the edges. The previous scheme ran 512
/// random cases because that count made one run likely (~94%) to hit every
/// enumerated edge case; enumeration makes that guarantee exact and much cheaper.
pub const RANDOM_TAIL_CASES: u32 = 64;

/// A numeric test plan: enumerated edge cases plus a strategy for the random tail.
pub struct NumericCases<V> {
    /// Edge cases; each runs exactly once.
    pub edges: Vec<V>,
    /// The strategy for the random tail; runs [`RANDOM_TAIL_CASES`] times.
    pub random: BoxedStrategy<V>,
}

impl<V: core::fmt::Debug + Clone> NumericCases<V> {
    /// Runs `test` once for each edge case, then for the random tail.
    pub fn run(self, test: impl Fn(V) -> Result<(), TestCaseError>) {
        for case in &self.edges {
            if let Err(err) = test(case.clone()) {
                panic!("edge case {case:?} failed: {err}");
            }
        }
        match TestRunner::new(Config::with_cases(RANDOM_TAIL_CASES)).run(&self.random, test) {
            Ok(()) => (),
            Err(TestError::Fail(reason, value)) => {
                panic!("Found minimal(shrinked) failing case: {value:?}\nFailure: {reason:?}");
            }
            Err(err) => panic!("Unexpected test result: {err:?}"),
        }
    }
}

/// Builds numeric test plans that are biased toward edge cases like zero, one, max, min,
/// half, etc. Particularly useful for testing overflowing, checked, and wrapping arithmetic
/// operations.
///
/// Each associated function returns a [`NumericCases`] whose edge cases run exactly once and
/// whose random tail covers the values between the edges.
pub struct NumericStrategy<T> {
    _marker: PhantomData<T>,
}

impl<T> NumericStrategy<T>
where
    T: PrimInt + Arbitrary + 'static,
    std::ops::RangeInclusive<T>: Strategy<Value = T>,
{
    pub fn add_unsigned() -> NumericCases<(T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.one, v.max),
                (v.max, v.max),
                (v.half, v.half),
                (v.half, v.half_plus_one),
                (v.half_plus_one, v.half),
                (v.half_plus_one, v.half_plus_one),
                (v.max, v.zero),
                (v.zero, v.max),
                (v.zero, v.zero),
                (v.one, v.zero),
                (v.zero, v.one),
                (v.two, v.max),
                (v.max, v.two),
                (v.three, v.three),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn add_signed() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.one, v.max),
                (v.max, v.max),
                (v.min, neg_one),
                (neg_one, v.min),
                (v.min, v.min),
                (v.half, v.half_plus_one),
                (v.half_plus_one, v.half),
                (v.zero, v.zero),
                (v.max, v.zero),
                (v.min, v.zero),
                (v.zero, v.max),
                (v.zero, v.min),
                (v.max, neg_one),
                (neg_one, v.max),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn sub_unsigned() -> NumericCases<(T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        NumericCases {
            edges: vec![
                (v.zero, v.one),
                (v.zero, v.max),
                (v.max, v.max),
                (v.max, v.zero),
                (v.max, v.one),
                (v.half, v.half),
                (v.half_plus_one, v.half),
                (v.half, v.half_plus_one),
                (v.one, v.one),
                (v.zero, v.zero),
                (v.one, v.max),
                (v.two, v.max),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn sub_signed() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.min, v.one),
                (v.min, v.max),
                (v.max, v.min),
                (v.max, neg_one),
                (neg_one, v.max),
                (v.min, neg_one),
                (v.zero, v.min),
                (v.max, v.max),
                (v.min, v.min),
                (v.zero, v.zero),
                (v.max, v.zero),
                (v.min, v.zero),
                (v.zero, v.max),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn mul_unsigned() -> NumericCases<(T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        NumericCases {
            edges: vec![
                (v.max, v.two),
                (v.two, v.max),
                (v.max, v.max),
                (v.half, v.two),
                (v.two, v.half),
                (v.half_plus_one, v.two),
                (v.two, v.half_plus_one),
                (v.max, v.one),
                (v.one, v.max),
                (v.max, v.zero),
                (v.zero, v.max),
                (v.zero, v.zero),
                (v.one, v.one),
                (v.two, v.two),
                (v.three, v.three),
                (v.half, v.half),
                (v.sqrt_max, v.sqrt_max),
                (v.sqrt_max, v.sqrt_max_plus_one),
                (v.sqrt_max_plus_one, v.sqrt_max),
                (v.sqrt_max_plus_one, v.sqrt_max_plus_one),
                (v.max_div_three, v.three),
                (v.three, v.max_div_three),
                (v.max_div_three_plus_one, v.three),
                (v.three, v.max_div_three_plus_one),
                (v.max_div_four, v.four),
                (v.four, v.max_div_four),
                (v.max_div_four_plus_one, v.four),
                (v.four, v.max_div_four_plus_one),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn mul_signed() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        let neg_two = v.zero - v.two;
        let neg_three = v.zero - v.three;
        let neg_four = v.zero - v.four;
        let neg_sqrt_max = v.zero - v.sqrt_max;
        let neg_sqrt_max_plus_one = v.zero - v.sqrt_max_plus_one;
        let neg_max_div_two = v.zero - v.half;
        let neg_max_div_two_plus_one = v.zero - v.half_plus_one;
        let neg_max_div_three = v.zero - v.max_div_three;
        let neg_max_div_three_plus_one = v.zero - v.max_div_three_plus_one;
        let neg_max_div_four = v.zero - v.max_div_four;
        let neg_max_div_four_plus_one = v.zero - v.max_div_four_plus_one;
        let min_div_two = v.min / v.two;
        let min_div_two_minus_one = min_div_two - v.one;
        let min_div_three = v.min / v.three;
        let min_div_three_minus_one = min_div_three - v.one;
        let min_div_four = v.min / v.four;
        let min_div_four_minus_one = min_div_four - v.one;
        NumericCases {
            edges: vec![
                (v.max, v.two),
                (v.two, v.max),
                (v.max, v.max),
                (v.half, v.two),
                (v.two, v.half),
                (v.half_plus_one, v.two),
                (v.two, v.half_plus_one),
                (v.max, v.one),
                (v.one, v.max),
                (v.min, v.one),
                (v.one, v.min),
                (v.max, v.zero),
                (v.zero, v.max),
                (v.min, v.zero),
                (v.zero, v.min),
                (v.zero, v.zero),
                (v.one, v.one),
                (v.two, v.two),
                (v.three, v.three),
                (v.min, neg_one),
                (neg_one, v.min),
                (v.max, neg_one),
                (neg_one, v.max),
                (v.min, v.two),
                (v.two, v.min),
                (v.min, neg_two),
                (neg_two, v.min),
                (v.min, v.three),
                (v.min, neg_three),
                (v.max, neg_two),
                (neg_two, v.max),
                (v.sqrt_max, v.sqrt_max),
                (v.sqrt_max, v.sqrt_max_plus_one),
                (v.sqrt_max_plus_one, v.sqrt_max),
                (v.sqrt_max_plus_one, v.sqrt_max_plus_one),
                (neg_sqrt_max, neg_sqrt_max),
                (neg_sqrt_max, neg_sqrt_max_plus_one),
                (neg_sqrt_max_plus_one, neg_sqrt_max),
                (neg_sqrt_max_plus_one, neg_sqrt_max_plus_one),
                (v.max_div_three, v.three),
                (v.three, v.max_div_three),
                (v.max_div_three_plus_one, v.three),
                (v.three, v.max_div_three_plus_one),
                (v.max_div_four, v.four),
                (v.four, v.max_div_four),
                (v.max_div_four_plus_one, v.four),
                (v.four, v.max_div_four_plus_one),
                (neg_max_div_two, neg_two),
                (neg_two, neg_max_div_two),
                (neg_max_div_two_plus_one, neg_two),
                (neg_two, neg_max_div_two_plus_one),
                (neg_max_div_three, neg_three),
                (neg_three, neg_max_div_three),
                (neg_max_div_three_plus_one, neg_three),
                (neg_three, neg_max_div_three_plus_one),
                (neg_max_div_four, neg_four),
                (neg_four, neg_max_div_four),
                (neg_max_div_four_plus_one, neg_four),
                (neg_four, neg_max_div_four_plus_one),
                (min_div_two, v.two),
                (v.two, min_div_two),
                (min_div_two_minus_one, v.two),
                (v.two, min_div_two_minus_one),
                (min_div_three, v.three),
                (v.three, min_div_three),
                (min_div_three_minus_one, v.three),
                (v.three, min_div_three_minus_one),
                (min_div_four, v.four),
                (v.four, min_div_four),
                (min_div_four_minus_one, v.four),
                (v.four, min_div_four_minus_one),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    /// Checked remainder and division don't panic on zero rhs.
    pub fn div_unsigned_checked() -> NumericCases<(T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.max, v.two),
                (v.max, v.max),
                (v.one, v.max),
                (v.zero, v.one),
                (v.zero, v.max),
                (v.half, v.two),
                (v.half_plus_one, v.two),
                (v.two, v.max),
                (v.max, v.zero),
                (v.zero, v.zero),
                (v.one, v.zero),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn div_unsigned_overflowing() -> NumericCases<(T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.max, v.two),
                (v.max, v.max),
                (v.one, v.max),
                (v.zero, v.one),
                (v.zero, v.max),
                (v.half, v.two),
                (v.half_plus_one, v.two),
                (v.two, v.max),
                (v.three, v.max),
            ],
            random: (any::<T>(), v.one..=v.max).boxed(),
        }
    }

    /// Checked remainder and division don't panic on zero rhs.
    pub fn div_signed_checked() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.max, neg_one),
                (v.min, v.one),
                (v.min, neg_one),
                (v.min, v.two),
                (v.max, v.two),
                (v.zero, v.one),
                (v.zero, v.min),
                (v.max, v.zero),
                (v.min, v.zero),
                (v.zero, v.zero),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn div_signed_overflowing() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.max, neg_one),
                (v.min, v.one),
                (v.min, neg_one),
                (v.min, v.two),
                (v.max, v.two),
                (v.zero, v.one),
                (v.zero, v.min),
                (neg_one, v.min),
                (neg_one, v.max),
            ],
            random: prop_oneof![
                3 => (any::<T>(), v.min..=neg_one),
                3 => (any::<T>(), v.one..=v.max),
            ]
            .boxed(),
        }
    }

    /// Checked remainder and division don't panic on zero rhs.
    pub fn rem_unsigned_checked() -> NumericCases<(T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.max, v.two),
                (v.max, v.max),
                (v.one, v.max),
                (v.zero, v.one),
                (v.zero, v.max),
                (v.half, v.two),
                (v.half_plus_one, v.two),
                (v.max, v.zero),
                (v.zero, v.zero),
                (v.one, v.zero),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn rem_unsigned_overflowing() -> NumericCases<(T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.max, v.two),
                (v.max, v.max),
                (v.one, v.max),
                (v.zero, v.one),
                (v.zero, v.max),
                (v.half, v.two),
                (v.half_plus_one, v.two),
                (v.two, v.max),
            ],
            random: (any::<T>(), v.one..=v.max).boxed(),
        }
    }

    /// Checked remainder and division don't panic on zero rhs.
    pub fn rem_signed_checked() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.max, neg_one),
                (v.min, v.one),
                (v.min, neg_one),
                (v.min, v.two),
                (v.max, v.two),
                (v.zero, v.one),
                (v.zero, v.min),
                (v.one, v.min),
                (v.two, v.min),
                (v.max, v.zero),
                (v.min, v.zero),
                (v.zero, v.zero),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn rem_signed_overflowing() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.max, v.one),
                (v.max, neg_one),
                (v.min, v.one),
                (v.min, neg_one),
                (v.min, v.two),
                (v.max, v.two),
                (v.zero, v.one),
                (v.zero, v.min),
                (neg_one, v.min),
                (neg_one, v.max),
            ],
            random: prop_oneof![
                3 => (any::<T>(), v.min..=neg_one),
                3 => (any::<T>(), v.one..=v.max),
            ]
            .boxed(),
        }
    }

    pub fn is_signed() -> NumericCases<T>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        NumericCases {
            edges: vec![v.zero, v.one, v.neg_one.unwrap(), v.max, v.min, v.half, v.half_plus_one],
            random: any::<T>().boxed(),
        }
    }

    /// Does *not* return `T::min_value` because it traps miden vm.
    pub fn unchecked_neg() -> NumericCases<T>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        let min_plus_one = v.min + T::one();
        NumericCases {
            edges: vec![v.zero, v.one, neg_one, v.max, v.half, v.half_plus_one, min_plus_one],
            random: ((v.min + T::one())..=v.max).boxed(),
        }
    }

    pub fn comparison_signed() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.zero, v.zero),
                (v.one, v.one),
                (neg_one, neg_one),
                (v.max, v.max),
                (v.min, v.min),
                (v.zero, v.one),
                (v.one, v.zero),
                (neg_one, v.zero),
                (v.zero, neg_one),
                (v.max, neg_one),
                (neg_one, v.max),
                (v.min, v.one),
                (v.one, v.min),
                (v.min, v.max),
                (v.max, v.min),
                (v.half, v.half_plus_one),
                (v.half_plus_one, v.half),
                (v.zero, v.max),
                (v.max, v.zero),
                (v.zero, v.min),
                (v.min, v.zero),
                (v.one, v.max),
                (v.max, v.one),
            ],
            random: (any::<T>(), any::<T>()).boxed(),
        }
    }

    pub fn pow2_signed() -> NumericCases<T>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_exp = T::from(bit_width - 2).unwrap();
        let max_exp_plus_one = max_exp + T::one();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                // valid exponents
                v.zero,
                v.one,
                max_exp,
                // invalid exponents
                v.min,
                neg_one,
                max_exp_plus_one,
                v.max,
            ],
            random: prop_oneof![
                // valid exponents
                2 => v.zero..=max_exp,
                // invalid exponents
                1 => v.min..=neg_one,
                1 => max_exp_plus_one..=v.max,
            ]
            .boxed(),
        }
    }

    pub fn ipow_signed() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let thirty = T::from(30).unwrap();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.zero, v.zero),
                (v.one, v.zero),
                (neg_one, v.zero),
                (v.max, v.zero),
                (v.min, v.zero),
                (v.zero, v.one),
                (v.one, v.one),
                (neg_one, v.one),
                (v.max, v.one),
                (v.min, v.one),
                (v.zero, v.two),
                (v.one, v.two),
                (neg_one, v.two),
                (v.max, v.two),
                (v.min, v.two),
                (v.zero, thirty),
                (v.one, thirty),
                (neg_one, thirty),
                (v.max, thirty),
                (v.min, thirty),
            ],
            random: (any::<T>(), v.zero..=thirty).boxed(),
        }
    }

    /// Out-of-range shift counts are covered by the bit width itself and by `T::MAX`, since
    /// unsigned types have no negative counts to exercise.
    pub fn shr_unsigned_checked() -> NumericCases<(T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = T::from(bit_width - 1).unwrap();
        let overflow_shift = T::from(bit_width).unwrap();
        NumericCases {
            edges: vec![
                (v.zero, v.zero),
                (v.zero, v.one),
                (v.zero, max_shift),
                (v.one, v.zero),
                (v.one, max_shift),
                (v.half, v.one),
                (v.half_plus_one, v.one),
                (v.half_plus_one, max_shift),
                (v.max, v.zero),
                (v.max, v.one),
                (v.max, max_shift),
                (v.zero, overflow_shift),
                (v.one, overflow_shift),
                (v.half_plus_one, overflow_shift),
                (v.max, overflow_shift),
                (v.max, v.max),
            ],
            random: prop_oneof![
                3 => (any::<T>(), v.zero..=max_shift),
                3 => (any::<T>(), any::<T>()),
            ]
            .boxed(),
        }
    }

    pub fn shr_signed_checked() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = T::from(bit_width - 1).unwrap();
        let overflow_shift = T::from(bit_width).unwrap();
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.min, v.zero),
                (v.min, v.one),
                (v.min, max_shift),
                (v.max, v.zero),
                (v.max, max_shift),
                (neg_one, v.one),
                (neg_one, max_shift),
                (v.zero, v.zero),
                (v.zero, v.one),
                (v.zero, max_shift),
                (v.one, max_shift),
                (v.min, overflow_shift),
                (v.max, overflow_shift),
                (v.zero, neg_one),
                (v.zero, overflow_shift),
                (v.min, neg_one),
                (v.max, neg_one),
            ],
            random: prop_oneof![
                3 => (any::<T>(), v.zero..=max_shift),
                3 => (any::<T>(), any::<T>()),
            ]
            .boxed(),
        }
    }

    /// The shift amount (second tuple value) is bound by `u32::MAX`.
    pub fn shr_signed_checked_u32_shift() -> NumericCases<(T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = T::from(bit_width - 1).unwrap();
        let overflow_shift = T::from(bit_width).unwrap();
        let max_u32_shift = T::from(u32::MAX).unwrap_or(v.max);
        let neg_one = v.neg_one.unwrap();
        NumericCases {
            edges: vec![
                (v.min, v.zero),
                (v.min, v.one),
                (v.min, max_shift),
                (v.max, v.zero),
                (v.max, max_shift),
                (neg_one, v.one),
                (neg_one, max_shift),
                (v.zero, v.zero),
                (v.zero, v.one),
                (v.zero, max_shift),
                (v.one, max_shift),
                (v.min, overflow_shift),
                (v.max, overflow_shift),
                (v.zero, overflow_shift),
                (v.min, max_u32_shift),
                (v.max, max_u32_shift),
                (v.zero, max_u32_shift),
            ],
            random: prop_oneof![
                3 => (any::<T>(), v.zero..=max_shift),
                3 => (any::<T>(), overflow_shift..=max_u32_shift),
            ]
            .boxed(),
        }
    }

    /// Shift amount (second tuple value) is a `u32`, matching `rotate_left`/`rotate_right`. Rotates
    /// are total and reduce the count modulo the operand width, so identity points (multiples of
    /// the width) and out-of-range counts are edge cases.
    pub fn rotate_unsigned_u32() -> NumericCases<(T, u32)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = bit_width - 1;
        let overflow_shift = bit_width;
        let double_width = bit_width * 2;
        // 0x55.. and 0xAA.. bit patterns expose rotate bugs that uniform bytes miss.
        let alt_lo = v.max_div_three;
        let alt_hi = v.max_div_three + v.max_div_three;
        NumericCases {
            edges: vec![
                (v.max, 0u32),
                (v.max, 1u32),
                (v.max, max_shift),
                (v.max, overflow_shift),
                (v.one, max_shift),
                (v.one, overflow_shift),
                (alt_lo, 1u32),
                (alt_lo, max_shift),
                (alt_hi, 1u32),
                (alt_hi, max_shift),
                (v.half, max_shift),
                (v.half_plus_one, max_shift),
                (v.max, double_width),
                (v.max, u32::MAX),
                (v.zero, overflow_shift),
            ],
            random: prop_oneof![
                3 => (any::<T>(), 0u32..bit_width),
                3 => (any::<T>(), bit_width..=u32::MAX),
            ]
            .boxed(),
        }
    }

    /// Signed counterpart of [`Self::rotate_unsigned_u32`]; adds `min` (sign bit set) and `-1`
    /// (all-ones) operands.
    pub fn rotate_signed_u32() -> NumericCases<(T, u32)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = bit_width - 1;
        let overflow_shift = bit_width;
        let double_width = bit_width * 2;
        NumericCases {
            edges: vec![
                (v.min, 0u32),
                (v.min, 1u32),
                (v.min, max_shift),
                (v.min, overflow_shift),
                (v.max, 1u32),
                (v.max, max_shift),
                (neg_one, 1u32),
                (neg_one, max_shift),
                (neg_one, overflow_shift),
                (v.one, max_shift),
                (v.one, overflow_shift),
                (v.min, double_width),
                (v.max, u32::MAX),
                (v.zero, overflow_shift),
            ],
            random: prop_oneof![
                3 => (any::<T>(), 0u32..bit_width),
                3 => (any::<T>(), bit_width..=u32::MAX),
            ]
            .boxed(),
        }
    }

    /// Shift amount is a `u32`, for `checked_shl`/`checked_shr`, which return `None` once the shift
    /// is `>= width`. The last in-range shift (`width - 1`) and first out-of-range shift (`width`)
    /// are edge cases.
    pub fn checked_shift_unsigned_u32() -> NumericCases<(T, u32)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = bit_width - 1;
        let overflow_shift = bit_width;
        NumericCases {
            edges: vec![
                (v.max, 0u32),
                (v.max, 1u32),
                (v.max, max_shift),
                (v.max, overflow_shift),
                (v.max, overflow_shift + 1),
                (v.one, max_shift),
                (v.one, overflow_shift),
                (v.half, max_shift),
                (v.half_plus_one, max_shift),
                (v.zero, 0u32),
                (v.zero, overflow_shift),
                (v.max, u32::MAX),
            ],
            random: prop_oneof![
                3 => (any::<T>(), 0u32..bit_width),
                3 => (any::<T>(), bit_width..=u32::MAX),
            ]
            .boxed(),
        }
    }

    /// Signed counterpart of [`Self::checked_shift_unsigned_u32`].
    pub fn checked_shift_signed_u32() -> NumericCases<(T, u32)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = bit_width - 1;
        let overflow_shift = bit_width;
        NumericCases {
            edges: vec![
                (v.min, 0u32),
                (v.min, 1u32),
                (v.min, max_shift),
                (v.min, overflow_shift),
                (v.max, max_shift),
                (v.max, overflow_shift),
                (neg_one, 1u32),
                (neg_one, max_shift),
                (neg_one, overflow_shift),
                (v.one, max_shift),
                (v.zero, overflow_shift),
                (v.min, u32::MAX),
            ],
            random: prop_oneof![
                3 => (any::<T>(), 0u32..bit_width),
                3 => (any::<T>(), bit_width..=u32::MAX),
            ]
            .boxed(),
        }
    }

    /// Shift amount is a `u32`, for `overflowing_shl`/`overflowing_shr`. The boolean reports whether
    /// the shift was masked (i.e. was `>= width`), so the width boundary is an edge case.
    pub fn overflowing_shift_unsigned_u32() -> NumericCases<(T, u32)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = bit_width - 1;
        let overflow_shift = bit_width;
        let double_width = bit_width * 2;
        NumericCases {
            edges: vec![
                (v.max, 0u32),
                (v.max, max_shift),
                (v.max, overflow_shift),
                (v.max, overflow_shift + 1),
                (v.max, double_width),
                (v.one, max_shift),
                (v.one, overflow_shift),
                (v.half, max_shift),
                (v.half_plus_one, overflow_shift),
                (v.zero, overflow_shift),
                (v.max, u32::MAX),
            ],
            random: prop_oneof![
                3 => (any::<T>(), 0u32..bit_width),
                3 => (any::<T>(), bit_width..=u32::MAX),
            ]
            .boxed(),
        }
    }

    /// Signed counterpart of [`Self::overflowing_shift_unsigned_u32`].
    pub fn overflowing_shift_signed_u32() -> NumericCases<(T, u32)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = bit_width - 1;
        let overflow_shift = bit_width;
        let double_width = bit_width * 2;
        NumericCases {
            edges: vec![
                (v.min, 0u32),
                (v.min, max_shift),
                (v.min, overflow_shift),
                (v.max, max_shift),
                (v.max, overflow_shift),
                (neg_one, max_shift),
                (neg_one, overflow_shift),
                (v.one, overflow_shift),
                (v.min, double_width),
                (v.zero, overflow_shift),
                (v.min, u32::MAX),
            ],
            random: prop_oneof![
                3 => (any::<T>(), 0u32..bit_width),
                3 => (any::<T>(), bit_width..=u32::MAX),
            ]
            .boxed(),
        }
    }

    /// Shift amount is a `u32`, for `unbounded_shl`/`unbounded_shr`, which yield `0` (or the sign
    /// fill for signed `shr`) once the shift is `>= width`. Large out-of-range shifts are edge
    /// cases alongside the width boundary.
    pub fn unbounded_shift_unsigned_u32() -> NumericCases<(T, u32)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = bit_width - 1;
        let overflow_shift = bit_width;
        let double_width = bit_width * 2;
        NumericCases {
            edges: vec![
                (v.max, 0u32),
                (v.max, max_shift),
                (v.max, overflow_shift),
                (v.max, double_width),
                (v.one, max_shift),
                (v.one, overflow_shift),
                (v.half, max_shift),
                (v.half_plus_one, overflow_shift),
                (v.zero, overflow_shift),
                (v.max, u32::MAX),
                (v.max, u32::MAX - 1),
            ],
            random: prop_oneof![
                3 => (any::<T>(), 0u32..bit_width),
                3 => (any::<T>(), bit_width..=u32::MAX),
            ]
            .boxed(),
        }
    }

    /// Signed counterpart of [`Self::unbounded_shift_unsigned_u32`].
    pub fn unbounded_shift_signed_u32() -> NumericCases<(T, u32)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = bit_width - 1;
        let overflow_shift = bit_width;
        let double_width = bit_width * 2;
        NumericCases {
            edges: vec![
                (v.min, 0u32),
                (v.min, max_shift),
                (v.min, overflow_shift),
                (v.min, double_width),
                (neg_one, max_shift),
                (neg_one, overflow_shift),
                (v.max, overflow_shift),
                (v.one, overflow_shift),
                (v.zero, overflow_shift),
                (v.min, u32::MAX),
                (neg_one, u32::MAX),
            ],
            random: prop_oneof![
                3 => (any::<T>(), 0u32..bit_width),
                3 => (any::<T>(), bit_width..=u32::MAX),
            ]
            .boxed(),
        }
    }
}

/// Common values frequently used in [`NumericStrategy`].
pub struct NumericStrategyValues<T: PrimInt> {
    pub zero: T,
    pub one: T,
    pub two: T,
    pub three: T,
    pub four: T,
    pub half: T,
    pub half_plus_one: T,
    pub sqrt_max: T,
    pub sqrt_max_plus_one: T,
    pub max_div_three: T,
    pub max_div_three_plus_one: T,
    pub max_div_four: T,
    pub max_div_four_plus_one: T,
    pub max: T,
    pub min: T,
    /// Only signed types can have negative values.
    pub neg_one: Option<T>,
}

impl<T: PrimInt> NumericStrategyValues<T> {
    pub fn new() -> Self {
        let two = T::one() + T::one();
        let three = two + T::one();
        let four = two + two;
        let max = T::max_value();
        let sqrt_max = integer_sqrt(max);
        let is_signed = T::min_value() < T::zero();
        Self {
            zero: T::zero(),
            one: T::one(),
            two,
            three,
            four,
            max,
            min: T::min_value(),
            half: max / two,
            half_plus_one: max / two + T::one(),
            sqrt_max,
            sqrt_max_plus_one: sqrt_max + T::one(),
            max_div_three: max / three,
            max_div_three_plus_one: max / three + T::one(),
            max_div_four: max / four,
            max_div_four_plus_one: max / four + T::one(),
            neg_one: is_signed.then(|| T::zero() - T::one()),
        }
    }
}

pub fn integer_sqrt<T: PrimInt>(n: T) -> T {
    let zero = T::zero();
    let one = T::one();
    let two = one + one;
    let mut low = one;
    let mut high = n;
    let mut result = zero;

    while low <= high {
        let mid = low + (high - low) / two;
        if mid <= n / mid {
            result = mid;
            low = mid + one;
        } else {
            high = mid - one;
        }
    }

    result
}
