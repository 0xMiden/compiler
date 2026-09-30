//! Tests for the lowering of `hir.mem_cpy`, including overlapping source and destination ranges
//! and the range and alignment traps.
//!
//! Regression tests for #1418.

use std::{rc::Rc, sync::Arc};

use midenc_hir::ArrayType;

use super::*;
use crate::trap_helpers::{panic_message, trap_matches};

/// The default size in bytes of the memory region a test case initializes and checks.
const REGION_LEN: usize = 64;

/// The size in bytes of each guard band placed directly below and above the region; a multiple of
/// 16, so the region keeps the alignment of the buffer.
const GUARD: usize = 32;

/// The value of every guard band byte; it does not occur in the first 206 bytes of
/// [patterned_region] or [felt_region].
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

/// Returns `len` bytes of non-trivial region contents for the table-driven tests.
fn patterned_region(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(7).wrapping_add(3)).collect()
}

/// Returns [patterned_region] with the top bit of every 4th byte cleared, so that every 4-byte
/// group, read as a little-endian `u32`, is a valid field element below 2^31.
fn felt_region(len: usize) -> Vec<u8> {
    let mut region = patterned_region(len);
    for byte in region.iter_mut().skip(3).step_by(4) {
        *byte &= 0x7f;
    }
    region
}

/// Reads `len` bytes of Rust memory starting at the element-aligned byte address `addr`, failing
/// if an element holds a value that does not fit in a `u32`.
fn read_rust_bytes(
    trace: &impl DebugQuery,
    addr: u32,
    len: usize,
) -> Result<Vec<u8>, TestCaseError> {
    let mut bytes = Vec::with_capacity(len.next_multiple_of(4));
    for index in 0..len.div_ceil(4) as u32 {
        let elem_addr = addr / 4 + index;
        // Untouched memory reads as zero in the VM
        let elem = trace.read_memory_element(elem_addr).unwrap_or_default().as_canonical_u64();
        let elem = u32::try_from(elem).map_err(|_| {
            TestCaseError::fail(format!(
                "element address {elem_addr} holds {elem}, which does not fit in u32"
            ))
        })?;
        bytes.extend(elem.to_le_bytes());
    }
    bytes.truncate(len);
    Ok(bytes)
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
    region: &[u8],
    case: Case,
) -> Result<(), TestCaseError> {
    let Case {
        src_off,
        dst_off,
        count,
    } = case;
    let elem_size = u32::try_from(elem.size_in_bytes()).expect("pointee size fits in u32");
    let len_bytes = (count * elem_size) as usize;
    let buffer_len = GUARD + region.len() + GUARD;
    let mut buffer = vec![GUARD_BYTE; buffer_len];
    buffer[GUARD..GUARD + region.len()].copy_from_slice(region);
    let mut expected = buffer.clone();
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
            let observed = read_rust_bytes(trace, base, buffer_len)?;
            prop_assert_eq!(
                &observed,
                &expected,
                "unexpected memory contents (region or guard bands) after mem_cpy of {} with \
                 src_off={}, dst_off={}, count={}, base={}",
                elem,
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

/// Runs every case of a table-driven test on the fixed base address with `region` as the initial
/// region contents.
fn check_table(elem: Type, region: &[u8], cases: &[Case]) {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_mem_cpy(elem.clone());
    for &case in cases {
        if let Err(err) = check_mem_cpy(&package, &context, &elem, FIXED_BASE, region, case) {
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
        &patterned_region(REGION_LEN),
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
        &patterned_region(REGION_LEN),
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
        &patterned_region(REGION_LEN),
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
        &patterned_region(REGION_LEN),
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
        &patterned_region(REGION_LEN),
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

/// Checks that 32-byte aggregates (`[u32; 8]`) are copied two words per value, several values per
/// copy, overlapping ranges included.
#[test]
fn mem_cpy_multiword_elements() {
    check_table(
        Type::from(ArrayType::new(Type::U32, 8)),
        &patterned_region(192),
        &[
            // Overlap, destination above source by one value
            case(0, 32, 4),
            // Overlap, destination below source by one value
            case(32, 0, 4),
            // Overlap, destination above source by 16 bytes (half a value)
            case(0, 16, 4),
            // Overlap, destination below source by 16 bytes (half a value)
            case(16, 0, 4),
            // Identical ranges
            case(32, 32, 3),
            // Disjoint, destination above source
            case(0, 96, 3),
            // Disjoint, destination below source
            case(96, 0, 3),
            // Zero count
            case(32, 64, 0),
        ],
    );
}

/// Checks that 1-byte pointees are copied as whole bytes on both the byte-loop and the element
/// paths, whatever the pointee type: `i1` values holding bytes other than 0 and 1 survive the copy
/// unchanged.
#[test]
fn mem_cpy_i1_elements() {
    check_table(
        Type::I1,
        &patterned_region(REGION_LEN),
        &[
            // Byte loop, overlap, destination above source by 1 byte
            case(3, 4, 21),
            // Byte loop, overlap, destination below source by 1 byte
            case(5, 4, 21),
            // Element path, overlap, destination above source by two elements
            case(0, 8, 16),
            // Element path, overlap, destination below source by two elements
            case(8, 0, 16),
            // Identical ranges
            case(5, 5, 23),
            // Zero count
            case(3, 7, 0),
        ],
    );
}

/// Checks `ptr<felt>` copies, which go one field element at a time, including overlapping ranges
/// in both directions.
#[test]
fn mem_cpy_felt_elements() {
    check_table(
        Type::Felt,
        &felt_region(REGION_LEN),
        &[
            // Overlap, destination above source by one value
            case(0, 4, 8),
            // Overlap, destination below source by one value
            case(4, 0, 8),
            // Identical ranges
            case(8, 8, 6),
            // Disjoint
            case(0, 32, 6),
            // Zero count
            case(4, 8, 0),
        ],
    );
}

/// Checks `ptr<u16>` copies, which go one sub-element value at a time, on both aligned and
/// unaligned addresses, including overlapping ranges in both directions.
#[test]
fn mem_cpy_u16_elements() {
    check_table(
        Type::U16,
        &patterned_region(REGION_LEN),
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
            check_mem_cpy(&package, &context, &Type::U8, base, &region, copy)
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

/// Checks random copies of u16, u32, u64 and u128 values (possibly overlapping) against
/// `copy_within`; the u16, u32 and u64 cases use arbitrary byte offsets, the u128 cases offsets
/// that are multiples of 16.
#[test]
fn mem_cpy_values_match_copy_within() {
    setup::enable_compiler_instrumentation();
    let compiled = [Type::U16, Type::U32, Type::U64, Type::U128].map(|elem| {
        let (package, context) = compile_mem_cpy(elem.clone());
        (elem, package, context)
    });

    let config = proptest::test_runner::Config::with_cases(32);
    let res = TestRunner::new(config).run(
        &(
            0..compiled.len(),
            any::<[u8; REGION_LEN]>(),
            random_word_aligned_addr(),
            0u32..REGION_LEN as u32,
            0u32..REGION_LEN as u32,
            any::<u32>(),
        ),
        move |(index, region, base, src_off, dst_off, count)| {
            let (elem, package, context) = &compiled[index];
            let elem_size = elem.size_in_bytes() as u32;
            // The word copy of u128 values requires 16-byte aligned addresses
            let (src_off, dst_off) = if elem_size == 16 {
                (16 * (src_off / 16), 16 * (dst_off / 16))
            } else {
                (src_off, dst_off)
            };
            // Keep both ranges inside the region
            let max_count = (REGION_LEN as u32 - src_off.max(dst_off)) / elem_size;
            let copy = case(src_off, dst_off, count % (max_count + 1));
            check_mem_cpy(package, context, elem, base, &region, copy)
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
