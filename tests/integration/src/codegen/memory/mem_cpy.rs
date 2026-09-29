//! Tests for the lowering of `hir.mem_cpy`, including overlapping source and destination ranges.
//!
//! Regression tests for #1418.

use std::{rc::Rc, sync::Arc};

use super::*;

/// The size in bytes of the memory region each test case initializes and checks.
const REGION_LEN: usize = 64;

/// A word-aligned byte address above the pages reserved for the Rust stack.
const FIXED_BASE: u32 = 17 * 2u32.pow(16);

/// A single `mem_cpy` case: the offsets are in bytes from the region base, `count` is in units of
/// the pointee type.
#[derive(Debug, Clone, Copy)]
struct Case {
    src_off: u32,
    dst_off: u32,
    count: u32,
}

/// Returns the non-trivial region contents used by the table-driven tests.
fn patterned_region() -> [u8; REGION_LEN] {
    core::array::from_fn(|i| (i as u8).wrapping_mul(7).wrapping_add(3))
}

/// Compiles a `main(src: ptr<elem>, dst: ptr<elem>, count: u32) -> u32` function which performs
/// `hir.mem_cpy src, dst, count` and returns `count`.
fn compile_mem_cpy(elem: Type) -> (Arc<miden_mast_package::Package>, Rc<midenc_hir::Context>) {
    let ptr_ty = Type::from(PointerType::new(elem));
    compile_test_module([ptr_ty.clone(), ptr_ty, Type::U32], [Type::U32], |builder| {
        let block = builder.current_block();
        let (src, dst, count) = {
            let block_ref = block.borrow();
            let args = block_ref.arguments();
            (args[0] as ValueRef, args[1] as ValueRef, args[2] as ValueRef)
        };
        builder.memcpy(src, dst, count, SourceSpan::default()).unwrap();
        builder.ret(Some(count), SourceSpan::default()).unwrap();
    })
}

/// Runs `case` against `package` on a region at `base` initialized with `region`, and checks that
/// the whole region afterwards equals the result of `copy_within` over the same bytes.
fn check_mem_cpy(
    package: &Arc<miden_mast_package::Package>,
    context: &Rc<midenc_hir::Context>,
    elem_size: u32,
    base: u32,
    region: [u8; REGION_LEN],
    case: Case,
) -> Result<(), TestCaseError> {
    let Case {
        src_off,
        dst_off,
        count,
    } = case;
    let len_bytes = (count * elem_size) as usize;
    let mut expected = region;
    expected.copy_within(src_off as usize..src_off as usize + len_bytes, dst_off as usize);

    let initializers = [Initializer::MemoryBytes {
        addr: base,
        bytes: &region,
    }];
    // C calling convention: first argument on top of the stack
    let args = [
        Felt::new_unchecked((base + src_off) as u64),
        Felt::new_unchecked((base + dst_off) as u64),
        Felt::new_unchecked(count as u64),
    ];
    let output = eval_package::<u32, _, _>(
        package.clone(),
        initializers,
        &args,
        context.session(),
        |trace| {
            let observed =
                trace.read_from_rust_memory::<[u8; REGION_LEN]>(base).ok_or_else(|| {
                    TestCaseError::fail(format!("failed to read from byte address {base}"))
                })?;
            prop_assert_eq!(
                observed,
                expected,
                "unexpected memory contents after mem_cpy with src_off={}, dst_off={}, count={}, \
                 base={}",
                src_off,
                dst_off,
                count,
                base
            );
            Ok(())
        },
    )?;
    prop_assert_eq!(output, count);
    Ok(())
}

/// Runs every case of a table-driven test on the fixed base address with the patterned region.
fn check_table(elem: Type, elem_size: u32, cases: &[Case]) {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_mem_cpy(elem);
    for &case in cases {
        if let Err(err) =
            check_mem_cpy(&package, &context, elem_size, FIXED_BASE, patterned_region(), case)
        {
            panic!("FAILURE for {case:?}: {err}");
        }
    }
}

/// Shorthand constructor for a [Case].
const fn case(src_off: u32, dst_off: u32, count: u32) -> Case {
    Case {
        src_off,
        dst_off,
        count,
    }
}

/// Checks byte copies where at least one of the offsets or the count is not a multiple of 4,
/// including overlapping ranges in both directions.
#[test]
fn mem_cpy_bytes_unaligned() {
    check_table(
        Type::U8,
        1,
        &[
            // Overlap, destination above source by 1 byte
            case(3, 4, 21),
            // Overlap, destination above source by several bytes
            case(3, 10, 41),
            // Overlap, destination below source by 1 byte
            case(5, 4, 21),
            // Overlap, destination below source by several bytes
            case(10, 3, 41),
            // Identical ranges
            case(5, 5, 23),
            // Disjoint, destination above source
            case(1, 40, 17),
            // Disjoint, destination below source
            case(40, 1, 17),
            case(3, 7, 0),
            case(3, 7, 1),
        ],
    );
}

/// Checks byte copies where both offsets and the count are multiples of 4, including overlapping
/// ranges in both directions.
#[test]
fn mem_cpy_bytes_element_aligned() {
    check_table(
        Type::U8,
        1,
        &[
            // Overlap, destination above source by one element
            case(4, 8, 40),
            // Overlap, destination above source by several elements
            case(0, 12, 48),
            // Overlap, destination below source by one element
            case(8, 4, 40),
            // Overlap, destination below source by several elements
            case(12, 0, 48),
            // Identical ranges
            case(8, 8, 32),
            // Disjoint, destination above source
            case(0, 32, 24),
            // Disjoint, destination below source
            case(32, 0, 24),
            case(4, 8, 0),
            case(4, 8, 4),
        ],
    );
}

/// Checks `ptr<u32>` copies, where `count` is a number of u32 values rather than bytes.
#[test]
fn mem_cpy_u32_elements() {
    check_table(
        Type::U32,
        4,
        &[
            // Overlap, destination above source by one value
            case(4, 8, 10),
            // Overlap, destination below source by one value
            case(8, 4, 10),
            // Identical ranges
            case(8, 8, 6),
            // Disjoint
            case(0, 32, 6),
            case(4, 8, 0),
        ],
    );
}

/// Checks random byte copies (possibly overlapping, aligned or not) against `copy_within`.
#[test]
fn mem_cpy_bytes_matches_copy_within() {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_mem_cpy(Type::U8);

    let config = proptest::test_runner::Config::with_cases(32);
    let res = TestRunner::new(config).run(
        &(
            any::<[u8; REGION_LEN]>(),
            random_word_aligned_addr(),
            0u32..32,
            0u32..32,
            0u32..=32,
        ),
        move |(region, base, src_off, dst_off, count)| {
            check_mem_cpy(&package, &context, 1, base, region, case(src_off, dst_off, count))
        },
    );

    match res {
        Err(TestError::Fail(reason, value)) => {
            panic!("FAILURE: {}\nMinimal failing case: {value:?}", reason.message());
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {res:?}"),
    }
}
