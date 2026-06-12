use std::{marker::PhantomData, sync::Arc};

use miden_assembly::{Assembler, DefaultSourceManager, Parse, ParseOptions, ast::ModuleKind};
use miden_core::Felt;
use miden_core_lib::CoreLibrary;
use miden_processor::{ExecutionError, Program, operation::OperationError};
use num_traits::{PrimInt, Unsigned};
use proptest::{
    prelude::*,
    test_runner::{Config, TestRunner},
};

use crate::compiler_test::{sdk_alloc_crate_path, sdk_crate_path};

const I32_INTRINSICS_MASM: &str = include_str!("../../../../codegen/masm/intrinsics/i32.masm");
const I64_INTRINSICS_MASM: &str = include_str!("../../../../codegen/masm/intrinsics/i64.masm");

/// Assembles an executable program that wraps `procedure_body` inside a procedure that is called
/// as entry point.
///
/// Both i32 and i64 intrinsics modules are statically linked so the body can call the intrinsics
/// of either type by their fully-qualified path (`::intrinsics::i32::*` or `::intrinsics::i64::*`).
pub(super) fn assemble_test_program(procedure_body: &str) -> Program {
    let source_manager = Arc::new(DefaultSourceManager::default());
    let core_library = CoreLibrary::default();

    // Parse both intrinsic modules with their fully-qualified paths
    let i32_intrinsics = I32_INTRINSICS_MASM
        .parse_with_options(
            source_manager.clone(),
            ParseOptions::new(ModuleKind::Library, "::intrinsics::i32"),
        )
        .expect("failed to parse i32 intrinsics module");
    let i64_intrinsics = I64_INTRINSICS_MASM
        .parse_with_options(
            source_manager.clone(),
            ParseOptions::new(ModuleKind::Library, "::intrinsics::i64"),
        )
        .expect("failed to parse i64 intrinsics module");

    // Parse the test module with its fully-qualified path
    let test_module_source = format!("pub proc test_intrinsic\n{procedure_body}\nend");
    let test_module = test_module_source
        .parse_with_options(
            source_manager.clone(),
            ParseOptions::new(ModuleKind::Library, "::test"),
        )
        .expect("failed to parse test module");

    // The i64 intrinsics module references core library symbols, so core must be available during
    // compile_and_statically_link.
    let mut assembler = Assembler::new(source_manager.clone())
        .with_static_library(core_library.library())
        .expect("failed to link core library");
    assembler
        .compile_and_statically_link(i32_intrinsics)
        .expect("failed to statically link i32 intrinsics");
    assembler
        .compile_and_statically_link(i64_intrinsics)
        .expect("failed to statically link i64 intrinsics");

    let library = assembler
        .assemble_library([test_module])
        .expect("failed to assemble test library");
    Assembler::new(source_manager)
        .with_static_library(library)
        .expect("failed to link library")
        .with_static_library(core_library.library())
        .expect("failed to link core library")
        .assemble_program(
            r#"
use miden::core::sys

begin
    exec.::test::test_intrinsic
    exec.sys::truncate_stack
end
"#,
        )
        .expect("failed to assemble program")
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

                [dependencies]
                miden-core = "*"
            "#,
    )
}

/// A strategy for generating pairs of numeric values, biased toward edge cases like
/// zero, one, max, min, half, etc. Particularly useful for testing overflowing,
/// checked, and wrapping arithmetic operations.
///
/// Associated strategies should distribute weights such that each edge case is likely to be
/// executed when run with a runner created by [`Self::test_runner`].
pub struct NumericStrategy<T> {
    _marker: PhantomData<T>,
}

impl<T> NumericStrategy<T> {
    /// Returns a test runner that generates enough cases to make each [`NumericStrategy`] edge case
    /// likely to be exercised.
    ///
    /// With 512 generated cases, each individual weight-1 edge case in the largest strategy is hit
    /// with ~99.9% probability. For the largest current strategy (71 weight-1 edge cases plus a
    /// weight-2 random arm), the chance of hitting all edge cases in one run is ~94%. For a smaller
    /// 20-edge-case strategy with the same weight-2 random arm, the chance of hitting all edge
    /// cases is >99.99%.
    ///
    /// Intuition: a specific edge case is very unlikely to be missed, but there are many edge
    /// cases that could be the one missed. The expected number of missed edge cases in the largest
    /// strategy is about 71 * (72 / 73)^512 = 0.06.
    pub(super) fn test_runner() -> TestRunner {
        TestRunner::new(Config::with_cases(512))
    }
}

impl<T> NumericStrategy<T>
where
    T: PrimInt + Arbitrary + 'static,
    std::ops::RangeInclusive<T>: Strategy<Value = T>,
{
    pub fn add_unsigned() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            5 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.one, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.half, v.half)),
            1 => Just((v.half, v.half_plus_one)),
            1 => Just((v.half_plus_one, v.half)),
            1 => Just((v.half_plus_one, v.half_plus_one)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.zero)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.two, v.max)),
            1 => Just((v.max, v.two)),
            1 => Just((v.three, v.three)),
        ]
    }

    pub fn add_signed() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            5 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.one, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.min, neg_one)),
            1 => Just((neg_one, v.min)),
            1 => Just((v.min, v.min)),
            1 => Just((v.half, v.half_plus_one)),
            1 => Just((v.half_plus_one, v.half)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.max, neg_one)),
            1 => Just((neg_one, v.max)),
        ]
    }

    pub fn sub_unsigned() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            5 => (any::<T>(), any::<T>()),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.max, v.one)),
            1 => Just((v.half, v.half)),
            1 => Just((v.half_plus_one, v.half)),
            1 => Just((v.half, v.half_plus_one)),
            1 => Just((v.one, v.one)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.max)),
            1 => Just((v.two, v.max)),
        ]
    }

    pub fn sub_signed() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            5 => (any::<T>(), any::<T>()),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, v.max)),
            1 => Just((v.max, v.min)),
            1 => Just((v.max, neg_one)),
            1 => Just((neg_one, v.max)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.max, v.max)),
            1 => Just((v.min, v.min)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.max)),
        ]
    }

    pub fn mul_unsigned() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            2 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.two)),
            1 => Just((v.two, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.two, v.half)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.half_plus_one)),
            1 => Just((v.max, v.one)),
            1 => Just((v.one, v.max)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.one)),
            1 => Just((v.two, v.two)),
            1 => Just((v.three, v.three)),
            1 => Just((v.half, v.half)),
            1 => Just((v.sqrt_max, v.sqrt_max)),
            1 => Just((v.sqrt_max, v.sqrt_max_plus_one)),
            1 => Just((v.sqrt_max_plus_one, v.sqrt_max)),
            1 => Just((v.sqrt_max_plus_one, v.sqrt_max_plus_one)),
            1 => Just((v.max_div_three, v.three)),
            1 => Just((v.three, v.max_div_three)),
            1 => Just((v.max_div_three_plus_one, v.three)),
            1 => Just((v.three, v.max_div_three_plus_one)),
            1 => Just((v.max_div_four, v.four)),
            1 => Just((v.four, v.max_div_four)),
            1 => Just((v.max_div_four_plus_one, v.four)),
            1 => Just((v.four, v.max_div_four_plus_one)),
        ]
    }

    pub fn mul_signed() -> impl Strategy<Value = (T, T)>
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
        prop_oneof![
            2 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.two)),
            1 => Just((v.two, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.two, v.half)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.half_plus_one)),
            1 => Just((v.max, v.one)),
            1 => Just((v.one, v.max)),
            1 => Just((v.min, v.one)),
            1 => Just((v.one, v.min)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.one)),
            1 => Just((v.two, v.two)),
            1 => Just((v.three, v.three)),
            1 => Just((v.min, neg_one)),
            1 => Just((neg_one, v.min)),
            1 => Just((v.max, neg_one)),
            1 => Just((neg_one, v.max)),
            1 => Just((v.min, v.two)),
            1 => Just((v.two, v.min)),
            1 => Just((v.min, neg_two)),
            1 => Just((neg_two, v.min)),
            1 => Just((v.min, v.three)),
            1 => Just((v.min, neg_three)),
            1 => Just((v.max, neg_two)),
            1 => Just((neg_two, v.max)),
            1 => Just((v.sqrt_max, v.sqrt_max)),
            1 => Just((v.sqrt_max, v.sqrt_max_plus_one)),
            1 => Just((v.sqrt_max_plus_one, v.sqrt_max)),
            1 => Just((v.sqrt_max_plus_one, v.sqrt_max_plus_one)),
            1 => Just((neg_sqrt_max, neg_sqrt_max)),
            1 => Just((neg_sqrt_max, neg_sqrt_max_plus_one)),
            1 => Just((neg_sqrt_max_plus_one, neg_sqrt_max)),
            1 => Just((neg_sqrt_max_plus_one, neg_sqrt_max_plus_one)),
            1 => Just((v.max_div_three, v.three)),
            1 => Just((v.three, v.max_div_three)),
            1 => Just((v.max_div_three_plus_one, v.three)),
            1 => Just((v.three, v.max_div_three_plus_one)),
            1 => Just((v.max_div_four, v.four)),
            1 => Just((v.four, v.max_div_four)),
            1 => Just((v.max_div_four_plus_one, v.four)),
            1 => Just((v.four, v.max_div_four_plus_one)),
            1 => Just((neg_max_div_two, neg_two)),
            1 => Just((neg_two, neg_max_div_two)),
            1 => Just((neg_max_div_two_plus_one, neg_two)),
            1 => Just((neg_two, neg_max_div_two_plus_one)),
            1 => Just((neg_max_div_three, neg_three)),
            1 => Just((neg_three, neg_max_div_three)),
            1 => Just((neg_max_div_three_plus_one, neg_three)),
            1 => Just((neg_three, neg_max_div_three_plus_one)),
            1 => Just((neg_max_div_four, neg_four)),
            1 => Just((neg_four, neg_max_div_four)),
            1 => Just((neg_max_div_four_plus_one, neg_four)),
            1 => Just((neg_four, neg_max_div_four_plus_one)),
            1 => Just((min_div_two, v.two)),
            1 => Just((v.two, min_div_two)),
            1 => Just((min_div_two_minus_one, v.two)),
            1 => Just((v.two, min_div_two_minus_one)),
            1 => Just((min_div_three, v.three)),
            1 => Just((v.three, min_div_three)),
            1 => Just((min_div_three_minus_one, v.three)),
            1 => Just((v.three, min_div_three_minus_one)),
            1 => Just((min_div_four, v.four)),
            1 => Just((v.four, min_div_four)),
            1 => Just((min_div_four_minus_one, v.four)),
            1 => Just((v.four, min_div_four_minus_one)),
        ]
    }

    /// Checked remainder and division don't panic on zero rhs.
    pub fn div_unsigned_checked() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            5 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, v.two)),
            1 => Just((v.max, v.max)),
            1 => Just((v.one, v.max)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.max)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.zero)),
        ]
    }

    pub fn div_unsigned_overflowing() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            5 => (any::<T>(), v.one..=v.max),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, v.two)),
            1 => Just((v.max, v.max)),
            1 => Just((v.one, v.max)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.max)),
            1 => Just((v.three, v.max)),
        ]
    }

    /// Checked remainder and division don't panic on zero rhs.
    pub fn div_signed_checked() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            5 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, neg_one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.min, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.zero)),
        ]
    }

    pub fn div_signed_overflowing() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            3 => (any::<T>(), v.min..=neg_one),
            3 => (any::<T>(), v.one..=v.max),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, neg_one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.min, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.min)),
            1 => Just((neg_one, v.min)),
            1 => Just((neg_one, v.max)),
        ]
    }

    /// Checked remainder and division don't panic on zero rhs.
    pub fn rem_unsigned_checked() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            5 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, v.two)),
            1 => Just((v.max, v.max)),
            1 => Just((v.one, v.max)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.zero)),
        ]
    }

    pub fn rem_unsigned_overflowing() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            5 => (any::<T>(), v.one..=v.max),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, v.two)),
            1 => Just((v.max, v.max)),
            1 => Just((v.one, v.max)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.max)),
        ]
    }

    /// Checked remainder and division don't panic on zero rhs.
    pub fn rem_signed_checked() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            5 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, neg_one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.min, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.zero)),
        ]
    }

    pub fn rem_signed_overflowing() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            3 => (any::<T>(), v.min..=neg_one),
            3 => (any::<T>(), v.one..=v.max),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, neg_one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.min, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.min)),
            1 => Just((neg_one, v.min)),
            1 => Just((neg_one, v.max)),
        ]
    }

    pub fn is_signed() -> impl Strategy<Value = T>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            5 => any::<T>(),
            1 => Just(v.zero),
            1 => Just(v.one),
            1 => Just(v.neg_one.unwrap()),
            1 => Just(v.max),
            1 => Just(v.min),
            1 => Just(v.half),
            1 => Just(v.half_plus_one),
        ]
    }

    /// Does *not* return `T::min_value` because it traps miden vm.
    pub fn unchecked_neg() -> impl Strategy<Value = T>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        let min_plus_one = v.min + T::one();
        prop_oneof![
            5 => (v.min+T::one())..=v.max,
            1 => Just(v.zero),
            1 => Just(v.one),
            1 => Just(neg_one),
            1 => Just(v.max),
            1 => Just(v.half),
            1 => Just(v.half_plus_one),
            1 => Just(min_plus_one),
        ]
    }

    pub fn comparison_signed() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            2 => (any::<T>(), any::<T>()),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.one)),
            1 => Just((neg_one, neg_one)),
            1 => Just((v.max, v.max)),
            1 => Just((v.min, v.min)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.one, v.zero)),
            1 => Just((neg_one, v.zero)),
            1 => Just((v.zero, neg_one)),
            1 => Just((v.max, neg_one)),
            1 => Just((neg_one, v.max)),
            1 => Just((v.min, v.one)),
            1 => Just((v.one, v.min)),
            1 => Just((v.min, v.max)),
            1 => Just((v.max, v.min)),
            1 => Just((v.half, v.half_plus_one)),
            1 => Just((v.half_plus_one, v.half)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.one, v.max)),
            1 => Just((v.max, v.one)),
        ]
    }

    pub fn pow2() -> impl Strategy<Value = T>
    where
        T: PrimInt + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        // The `pow2` intrinsics assert `n < bit_width - 1`.
        let max_exp = T::from(bit_width - 2).unwrap();
        prop_oneof![
            5 => v.zero..=max_exp,
            1 => Just(v.zero),
            1 => Just(v.one),
            1 => Just(max_exp),
        ]
    }

    pub fn ipow_signed() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let thirty = T::from(30).unwrap();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            2 => (any::<T>(), v.zero..=thirty),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.zero)),
            1 => Just((neg_one, v.zero)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.one, v.one)),
            1 => Just((neg_one, v.one)),
            1 => Just((v.max, v.one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.zero, v.two)),
            1 => Just((v.one, v.two)),
            1 => Just((neg_one, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.min, v.two)),
            1 => Just((v.zero, thirty)),
            1 => Just((v.one, thirty)),
            1 => Just((neg_one, thirty)),
            1 => Just((v.max, thirty)),
            1 => Just((v.min, thirty)),
        ]
    }

    pub fn shr_signed_checked() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = T::from(bit_width - 1).unwrap();
        let overflow_shift = T::from(bit_width).unwrap();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            3 => (any::<T>(), v.zero..=max_shift),
            3 => (any::<T>(), any::<T>()),
            1 => Just((v.min, v.zero)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, max_shift)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.max, max_shift)),
            1 => Just((neg_one, v.one)),
            1 => Just((neg_one, max_shift)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, max_shift)),
            1 => Just((v.one, max_shift)),
            1 => Just((v.min, overflow_shift)),
            1 => Just((v.max, overflow_shift)),
            1 => Just((v.zero, neg_one)),
            1 => Just((v.zero, overflow_shift)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.max, neg_one)),
        ]
    }

    /// The shift amount (second tuple value) is bound by `u32::MAX`.
    pub fn shr_signed_checked_u32_shift() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let bit_width = u32::try_from(std::mem::size_of::<T>() * 8).unwrap();
        let max_shift = T::from(bit_width - 1).unwrap();
        let overflow_shift = T::from(bit_width).unwrap();
        let max_u32_shift = T::from(u32::MAX).unwrap_or(v.max);
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            3 => (any::<T>(), v.zero..=max_shift),
            3 => (any::<T>(), overflow_shift..=max_u32_shift),
            1 => Just((v.min, v.zero)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, max_shift)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.max, max_shift)),
            1 => Just((neg_one, v.one)),
            1 => Just((neg_one, max_shift)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, max_shift)),
            1 => Just((v.one, max_shift)),
            1 => Just((v.min, overflow_shift)),
            1 => Just((v.max, overflow_shift)),
            1 => Just((v.zero, overflow_shift)),
            1 => Just((v.min, max_u32_shift)),
            1 => Just((v.max, max_u32_shift)),
            1 => Just((v.zero, max_u32_shift)),
        ]
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
