//! Tests for the lowering of `hir.mem_move`, including overlapping source and destination ranges
//! and the range and alignment traps.
//!
//! Regression tests for #1418.

use midenc_hir::ArrayType;

use super::{copy::*, *};

/// Checks byte copies where at least one of the offsets or the count is not a multiple of 4,
/// including overlapping ranges in both directions.
#[test]
fn mem_move_bytes_unaligned() {
    check_table(
        CopyOp::MemMove,
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
fn mem_move_bytes_element_aligned() {
    check_table(
        CopyOp::MemMove,
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
fn mem_move_u32_elements() {
    check_table(
        CopyOp::MemMove,
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
fn mem_move_u64_elements() {
    check_table(
        CopyOp::MemMove,
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
            // Adjacent, destination above source
            case(0, 32, 4),
            // Adjacent, destination below source
            case(32, 0, 4),
            // Zero count
            case(4, 8, 0),
        ],
    );
}

/// Checks `ptr<u128>` copies, which go a word at a time and require 16-byte aligned addresses,
/// including overlapping ranges in both directions.
#[test]
fn mem_move_u128_elements() {
    check_table(
        CopyOp::MemMove,
        Type::U128,
        &patterned_region(REGION_LEN),
        &[
            // Overlap, destination above source by one value
            case(0, 16, 3),
            // Overlap, destination below source by one value
            case(16, 0, 3),
            // Identical ranges
            case(16, 16, 2),
            // Adjacent, destination above source
            case(0, 32, 2),
            // Adjacent, destination below source
            case(32, 0, 2),
            // Zero count
            case(16, 32, 0),
        ],
    );
}

/// Checks that 32-byte aggregates (`[u32; 8]`) are copied two words per value, several values per
/// copy, overlapping ranges included.
#[test]
fn mem_move_multiword_elements() {
    check_table(
        CopyOp::MemMove,
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
            // Adjacent, destination above source
            case(0, 96, 3),
            // Adjacent, destination below source
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
fn mem_move_i1_elements() {
    check_table(
        CopyOp::MemMove,
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
fn mem_move_felt_elements() {
    check_table(
        CopyOp::MemMove,
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
fn mem_move_u16_elements() {
    check_table(
        CopyOp::MemMove,
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

/// Checks that a `ptr<u32>` copy whose byte length `count * 4` does not fit in `u32` traps with
/// the byte length overflow assertion.
#[test]
fn mem_move_byte_length_overflow_traps() {
    assert_copy_traps(
        CopyOp::MemMove,
        Type::U32,
        FIXED_BASE,
        FIXED_BASE + 32,
        0x4000_0000,
        "memmove byte length overflowed",
    );
}

/// Checks that a copy whose source or destination range extends past the end of the address
/// space traps with the matching range assertion.
#[test]
fn mem_move_out_of_range_traps() {
    assert_copy_traps(
        CopyOp::MemMove,
        Type::U8,
        u32::MAX - 7,
        FIXED_BASE,
        16,
        "memmove source range is out of bounds",
    );
    assert_copy_traps(
        CopyOp::MemMove,
        Type::U8,
        FIXED_BASE,
        u32::MAX - 7,
        16,
        "memmove destination range is out of bounds",
    );
}

/// Checks that a word copy (`ptr<u128>`) whose source or destination address is not 16-byte
/// aligned traps with the word-copy alignment assertion, also when `count` is zero.
#[test]
fn mem_move_word_copy_unaligned_traps() {
    const MESSAGE: &str = "expected a 16-byte-aligned byte pointer for the word-copy fast path";
    assert_copy_traps(CopyOp::MemMove, Type::U128, FIXED_BASE + 4, FIXED_BASE + 32, 1, MESSAGE);
    assert_copy_traps(CopyOp::MemMove, Type::U128, FIXED_BASE, FIXED_BASE + 4, 1, MESSAGE);
    assert_copy_traps(CopyOp::MemMove, Type::U128, FIXED_BASE + 4, FIXED_BASE + 32, 0, MESSAGE);
}

/// Checks random byte copies (possibly overlapping, aligned or not) against `copy_within`; about
/// half of the cases have both offsets and the count a multiple of 4.
#[test]
fn mem_move_bytes_matches_copy_within() {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_copy(CopyOp::MemMove, Type::U8);

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
            check_copy(CopyOp::MemMove, &package, &context, &Type::U8, base, &region, copy)
        },
    );
    assert_proptest_passed(res);
}

/// Checks random copies of u16, u32, u64 and u128 values (possibly overlapping) against
/// `copy_within`; the u16, u32 and u64 cases use arbitrary byte offsets, the u128 cases offsets
/// that are multiples of 16.
#[test]
fn mem_move_values_match_copy_within() {
    setup::enable_compiler_instrumentation();
    let compiled = [Type::U16, Type::U32, Type::U64, Type::U128].map(|elem| {
        let (package, context) = compile_copy(CopyOp::MemMove, elem.clone());
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
            check_copy(CopyOp::MemMove, package, context, elem, base, &region, copy)
        },
    );
    assert_proptest_passed(res);
}
