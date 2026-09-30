//! Tests for the lowering of `hir.mem_cpy`, including overlapping source and destination ranges
//! and the range and alignment traps.
//!
//! Regression tests for #1418.

use std::{rc::Rc, sync::Arc};

use midenc_hir::ArrayType;

use super::*;
use crate::trap_helpers::{panic_message, trap_matches};

/// The size in bytes of the memory region each test case initializes and checks.
const REGION_LEN: usize = 64;

/// The size in bytes of each guard band placed directly below and above the region; a multiple of
/// 16, so the region keeps the alignment of the buffer.
const GUARD: usize = 32;

/// The size in bytes of the whole buffer each test case initializes and checks: the region between
/// its two guard bands.
const BUFFER_LEN: usize = GUARD + REGION_LEN + GUARD;

/// The value of every guard band byte; it does not occur in [patterned_region].
const GUARD_BYTE: u8 = 0xa5;

/// A 16-byte aligned byte address above the pages reserved for the Rust stack.
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

/// Runs `case` against `package` on a region initialized with `region`, and checks that the region
/// afterwards equals the result of `copy_within` over the same bytes while the guard bands around
/// it are left untouched.
///
/// The buffer starts at `base` with a [GUARD]-byte band of [GUARD_BYTE], followed by the region and
/// another such band; the offsets of `case` are relative to the region start `base + GUARD`.
/// `elem` is the pointee type `package` was compiled for; it gives the size of one unit of
/// `case.count`.
fn check_mem_cpy(
    package: &Arc<miden_mast_package::Package>,
    context: &Rc<midenc_hir::Context>,
    elem: &Type,
    base: u32,
    region: [u8; REGION_LEN],
    case: Case,
) -> Result<(), TestCaseError> {
    let Case {
        src_off,
        dst_off,
        count,
    } = case;
    let elem_size = u32::try_from(elem.size_in_bytes()).expect("pointee size fits in u32");
    let len_bytes = (count * elem_size) as usize;
    let mut buffer = [GUARD_BYTE; BUFFER_LEN];
    buffer[GUARD..GUARD + REGION_LEN].copy_from_slice(&region);
    let mut expected = buffer;
    let (src, dst) = (GUARD + src_off as usize, GUARD + dst_off as usize);
    expected.copy_within(src..src + len_bytes, dst);

    let initializers = [Initializer::MemoryBytes {
        addr: base,
        bytes: &buffer,
    }];
    let region_base = base + GUARD as u32;
    // C calling convention: first argument on top of the stack
    let args = [
        Felt::new_unchecked((region_base + src_off) as u64),
        Felt::new_unchecked((region_base + dst_off) as u64),
        Felt::new_unchecked(count as u64),
    ];
    let output = eval_package::<u32, _, _>(
        package.clone(),
        initializers,
        &args,
        context.session(),
        |trace| {
            let observed =
                trace.read_from_rust_memory::<[u8; BUFFER_LEN]>(base).ok_or_else(|| {
                    TestCaseError::fail(format!("failed to read from byte address {base}"))
                })?;
            prop_assert_eq!(
                observed,
                expected,
                "unexpected memory contents (region or guard bands) after mem_cpy with \
                 src_off={}, dst_off={}, count={}, base={}",
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
fn check_table(elem: Type, cases: &[Case]) {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_mem_cpy(elem.clone());
    for &case in cases {
        if let Err(err) =
            check_mem_cpy(&package, &context, &elem, FIXED_BASE, patterned_region(), case)
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
            // Zero count
            case(3, 7, 0),
            // Single byte
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
            // Zero count
            case(4, 8, 0),
            // Single element
            case(4, 8, 4),
        ],
    );
}

/// Checks `ptr<u32>` copies, where `count` is a number of u32 values rather than bytes, on both
/// element-aligned and unaligned addresses.
#[test]
fn mem_cpy_u32_elements() {
    check_table(
        Type::U32,
        &[
            // Overlap, destination above source by one value
            case(4, 8, 10),
            // Overlap, destination below source by one value
            case(8, 4, 10),
            // Overlap, destination above source by 2 bytes
            case(4, 6, 5),
            // Overlap, destination below source by 2 bytes
            case(6, 4, 5),
            // Overlap, unaligned addresses, destination above source by 1 byte
            case(1, 2, 7),
            // Overlap, unaligned addresses, destination below source by 1 byte
            case(2, 1, 7),
            // Identical ranges
            case(8, 8, 6),
            // Disjoint
            case(0, 32, 6),
            // Zero count
            case(4, 8, 0),
        ],
    );
}

/// Checks `ptr<u64>` copies, where `count` is a number of u64 values rather than bytes, including
/// overlaps whose byte distance is not a multiple of the value size.
#[test]
fn mem_cpy_u64_elements() {
    check_table(
        Type::U64,
        &[
            // Overlap, destination above source by one value
            case(0, 8, 6),
            // Overlap, destination above source by 4 bytes
            case(4, 8, 5),
            // Overlap, unaligned addresses, destination above source by 3 bytes
            case(1, 4, 6),
            // Overlap, destination below source by one value
            case(8, 0, 6),
            // Overlap, destination below source by 4 bytes
            case(8, 4, 5),
            // Overlap, unaligned addresses, destination below source by 3 bytes
            case(4, 1, 6),
            // Identical ranges
            case(8, 8, 4),
            // Disjoint, destination above source
            case(0, 32, 4),
            // Disjoint, destination below source
            case(32, 0, 4),
            // Zero count
            case(4, 8, 0),
        ],
    );
}

/// Checks `ptr<u128>` copies, which go a word at a time and require 16-byte aligned addresses,
/// including overlapping ranges in both directions.
#[test]
fn mem_cpy_u128_elements() {
    check_table(
        Type::U128,
        &[
            // Overlap, destination above source by one value
            case(0, 16, 3),
            // Overlap, destination below source by one value
            case(16, 0, 3),
            // Identical ranges
            case(16, 16, 2),
            // Disjoint, destination above source
            case(0, 32, 2),
            // Disjoint, destination below source
            case(32, 0, 2),
            // Zero count
            case(16, 32, 0),
        ],
    );
}

/// Checks that a 32-byte aggregate (`[u32; 8]`) is copied two words per value, overlapping ranges
/// included.
#[test]
fn mem_cpy_multiword_elements() {
    check_table(
        Type::from(ArrayType::new(Type::U32, 8)),
        &[
            // Overlap, destination above source by 16 bytes (half a value)
            case(0, 16, 1),
            // Overlap, destination below source by 16 bytes (half a value)
            case(16, 0, 1),
            // Identical ranges
            case(16, 16, 1),
            // Disjoint, destination above source
            case(0, 32, 1),
            // Disjoint, destination below source
            case(32, 0, 1),
            // Zero count
            case(0, 16, 0),
        ],
    );
}

/// Checks `ptr<u16>` copies, which go one sub-element value at a time, on both aligned and
/// unaligned addresses, including overlapping ranges in both directions.
#[test]
fn mem_cpy_u16_elements() {
    check_table(
        Type::U16,
        &[
            // Overlap, destination above source by one value
            case(0, 2, 10),
            // Overlap, destination below source by one value
            case(2, 0, 10),
            // Overlap, unaligned source, destination above source by 3 bytes
            case(1, 4, 9),
            // Overlap, unaligned destination, destination below source by 3 bytes
            case(4, 1, 9),
            // Identical ranges
            case(6, 6, 5),
            // Disjoint
            case(0, 32, 8),
            // Zero count
            case(2, 4, 0),
        ],
    );
}

/// Asserts that `hir.mem_cpy` over `ptr<elem>` with the raw byte addresses `src` and `dst` and the
/// given `count` traps with the assertion `message`, see [trap_matches].
fn assert_mem_cpy_traps(elem: Type, src: u32, dst: u32, count: u32, message: &str) {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_mem_cpy(elem);
    let args = [
        Felt::new_unchecked(src as u64),
        Felt::new_unchecked(dst as u64),
        Felt::new_unchecked(count as u64),
    ];
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eval_package::<u32, _, _>(package.clone(), None, &args, context.session(), |_| Ok(()))
    }))
    .expect_err("mem_cpy should trap");
    let err = panic_message(panic);
    assert!(
        trap_matches(&err, message),
        "expected a trap containing {message:?} for src={src}, dst={dst}, count={count}, got: \
         {err}"
    );
}

/// Checks that a `ptr<u32>` copy whose byte length `count * 4` does not fit in `u32` traps with
/// the byte length overflow assertion.
#[test]
fn mem_cpy_byte_length_overflow_traps() {
    assert_mem_cpy_traps(
        Type::U32,
        FIXED_BASE,
        FIXED_BASE + 32,
        0x4000_0000,
        "memcpy byte length overflowed",
    );
}

/// Checks that a copy whose source or destination range extends past the end of the address
/// space traps with the matching range assertion.
#[test]
fn mem_cpy_out_of_range_traps() {
    assert_mem_cpy_traps(
        Type::U8,
        u32::MAX - 7,
        FIXED_BASE,
        16,
        "memcpy source range is out of bounds",
    );
    assert_mem_cpy_traps(
        Type::U8,
        FIXED_BASE,
        u32::MAX - 7,
        16,
        "memcpy destination range is out of bounds",
    );
}

/// Checks that a word copy (`ptr<u128>`) whose source or destination address is not 16-byte
/// aligned traps with the word-copy alignment assertion.
#[test]
fn mem_cpy_word_copy_unaligned_traps() {
    const MESSAGE: &str = "expected a 16-byte-aligned byte pointer for the word-copy fast path";
    assert_mem_cpy_traps(Type::U128, FIXED_BASE + 4, FIXED_BASE + 32, 1, MESSAGE);
    assert_mem_cpy_traps(Type::U128, FIXED_BASE, FIXED_BASE + 4, 1, MESSAGE);
}

/// Checks random byte copies (possibly overlapping, aligned or not) against `copy_within`; about
/// half of the cases have both offsets and the count a multiple of 4.
#[test]
fn mem_cpy_bytes_matches_copy_within() {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_mem_cpy(Type::U8);

    let config = proptest::test_runner::Config::with_cases(64);
    let res = TestRunner::new(config).run(
        &(
            any::<[u8; REGION_LEN]>(),
            random_word_aligned_addr(),
            any::<bool>(),
            0u32..32,
            0u32..32,
            0u32..=32,
        ),
        move |(region, base, element_aligned, src_off, dst_off, count)| {
            let copy = if element_aligned {
                case(4 * (src_off / 4), 4 * (dst_off / 4), 4 * (count / 4))
            } else {
                case(src_off, dst_off, count)
            };
            check_mem_cpy(&package, &context, &Type::U8, base, region, copy)
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
