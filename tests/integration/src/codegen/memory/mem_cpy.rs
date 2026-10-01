//! Tests for the lowering of `hir.mem_cpy`: copies between disjoint and adjacent source and
//! destination ranges, the trap on overlapping ranges, and the range and alignment traps.

use midenc_hir::ArrayType;

use super::{copy::*, *};

/// The message of the trap raised when the ranges of a `hir.mem_cpy` overlap.
const OVERLAP_MESSAGE: &str = "source and destination ranges must not overlap";

/// Returns true if the source and destination ranges of `case`, with values of `elem_size` bytes,
/// share at least one byte.
fn ranges_overlap(case: Case, elem_size: u32) -> bool {
    let len = case.count * elem_size;
    len != 0 && case.src_off < case.dst_off + len && case.dst_off < case.src_off + len
}

/// Returns overlapping cases of `count` values of `size` bytes around the offset `off`, where
/// `shift` is the smallest byte distance between the ranges the tested arm accepts: the
/// destination above and below the source by `shift` bytes, the destination above and below the
/// source with the ranges sharing exactly `shift` bytes, and identical ranges.
fn overlapping_cases(off: u32, shift: u32, size: u32, count: u32) -> [Case; 5] {
    let len = count * size;
    [
        case(off, off + shift, count),
        case(off, off + len - shift, count),
        case(off + shift, off, count),
        case(off + len - shift, off, count),
        case(off, off, count),
    ]
}

/// Asserts that `hir.mem_cpy` over `ptr<elem>` traps with [OVERLAP_MESSAGE] for every case, with
/// the offsets of the cases relative to [FIXED_BASE].
fn assert_overlap_traps(elem: Type, cases: &[Case]) {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_copy(CopyOp::MemCpy, elem);
    for &case in cases {
        let (src, dst) = (FIXED_BASE + case.src_off, FIXED_BASE + case.dst_off);
        if let Err(err) = check_copy_traps(
            CopyOp::MemCpy,
            &package,
            &context,
            src,
            dst,
            case.count,
            OVERLAP_MESSAGE,
        ) {
            panic!("FAILURE for {case:?}: {err}");
        }
    }
}

/// Checks byte copies where at least one of the offsets or the count is not a multiple of 4,
/// between disjoint and adjacent ranges in both directions, and with a zero count.
#[test]
fn mem_cpy_bytes_unaligned() {
    check_table(
        CopyOp::MemCpy,
        Type::U8,
        &patterned_region(REGION_LEN),
        &[
            // Disjoint, destination above source
            case(1, 40, 17),
            // Disjoint, destination below source
            case(40, 1, 17),
            // Adjacent, destination above source
            case(3, 20, 17),
            // Adjacent, destination below source
            case(20, 3, 17),
            // Zero count, identical pointers
            case(5, 5, 0),
            // Zero count, destination inside the source range a non-zero count would cover
            case(3, 7, 0),
            // Single byte
            case(3, 7, 1),
        ],
    );
}

/// Checks byte copies where both offsets and the count are multiples of 4, between disjoint and
/// adjacent ranges in both directions, and with a zero count.
#[test]
fn mem_cpy_bytes_element_aligned() {
    check_table(
        CopyOp::MemCpy,
        Type::U8,
        &patterned_region(REGION_LEN),
        &[
            // Disjoint, destination above source
            case(0, 32, 24),
            // Disjoint, destination below source
            case(32, 0, 24),
            // Adjacent, destination above source
            case(4, 28, 24),
            // Adjacent, destination below source
            case(28, 4, 24),
            // Zero count, identical pointers
            case(8, 8, 0),
            // Zero count, destination inside the source range a non-zero count would cover
            case(4, 8, 0),
            // Single element, adjacent
            case(4, 8, 4),
        ],
    );
}

/// Checks that 1-byte pointees are copied as whole bytes on both the byte-loop and the element
/// paths, whatever the pointee type: `i1` values holding bytes other than 0 and 1 survive the copy
/// unchanged.
#[test]
fn mem_cpy_i1_elements() {
    check_table(
        CopyOp::MemCpy,
        Type::I1,
        &patterned_region(REGION_LEN),
        &[
            // Byte loop, disjoint, destination above source
            case(1, 30, 21),
            // Byte loop, disjoint, destination below source
            case(30, 1, 21),
            // Byte loop, adjacent, destination above source
            case(3, 24, 21),
            // Byte loop, adjacent, destination below source
            case(24, 3, 21),
            // Element path, disjoint, destination above source
            case(0, 32, 16),
            // Element path, disjoint, destination below source
            case(32, 0, 16),
            // Element path, adjacent, destination above source
            case(0, 16, 16),
            // Element path, adjacent, destination below source
            case(16, 0, 16),
            // Byte loop, zero count, identical pointers
            case(5, 5, 0),
            // Element path, zero count, identical pointers
            case(8, 8, 0),
        ],
    );
}

/// Checks `ptr<u16>` copies, which go one sub-element value at a time, on both aligned and
/// unaligned addresses, between disjoint and adjacent ranges in both directions, and with a zero
/// count.
#[test]
fn mem_cpy_u16_elements() {
    check_table(
        CopyOp::MemCpy,
        Type::U16,
        &patterned_region(REGION_LEN),
        &[
            // Disjoint, destination above source
            case(0, 32, 8),
            // Disjoint, destination below source
            case(32, 0, 8),
            // Disjoint, unaligned addresses, destination above source
            case(1, 33, 9),
            // Disjoint, unaligned addresses, destination below source
            case(33, 1, 9),
            // Adjacent, destination above source
            case(0, 20, 10),
            // Adjacent, destination below source
            case(20, 0, 10),
            // Adjacent, unaligned addresses, destination above source
            case(3, 21, 9),
            // Adjacent, unaligned addresses, destination below source
            case(21, 3, 9),
            // Zero count, identical pointers
            case(6, 6, 0),
            // Zero count, destination inside the source range a non-zero count would cover
            case(2, 3, 0),
        ],
    );
}

/// Checks `ptr<u32>` copies, where `count` is a number of u32 values rather than bytes, on both
/// element-aligned and unaligned addresses, between disjoint and adjacent ranges in both
/// directions, and with a zero count.
#[test]
fn mem_cpy_u32_elements() {
    check_table(
        CopyOp::MemCpy,
        Type::U32,
        &patterned_region(REGION_LEN),
        &[
            // Disjoint, destination above source
            case(0, 32, 6),
            // Disjoint, destination below source
            case(32, 0, 6),
            // Disjoint, unaligned addresses, destination above source
            case(1, 34, 7),
            // Disjoint, unaligned addresses, destination below source
            case(34, 1, 7),
            // Adjacent, destination above source
            case(4, 28, 6),
            // Adjacent, destination below source
            case(28, 4, 6),
            // Adjacent, unaligned addresses, destination above source
            case(2, 30, 7),
            // Adjacent, unaligned addresses, destination below source
            case(30, 2, 7),
            // Zero count, identical pointers
            case(8, 8, 0),
            // Zero count, destination inside the source range a non-zero count would cover
            case(4, 6, 0),
        ],
    );
}

/// Checks `ptr<u64>` copies, where `count` is a number of u64 values rather than bytes, on both
/// element-aligned and unaligned addresses, between disjoint and adjacent ranges in both
/// directions, and with a zero count.
#[test]
fn mem_cpy_u64_elements() {
    check_table(
        CopyOp::MemCpy,
        Type::U64,
        &patterned_region(REGION_LEN),
        &[
            // Disjoint, destination above source
            case(0, 32, 3),
            // Disjoint, destination below source
            case(32, 0, 3),
            // Disjoint, unaligned addresses, destination above source
            case(1, 36, 3),
            // Disjoint, unaligned addresses, destination below source
            case(36, 1, 3),
            // Adjacent, destination above source
            case(8, 32, 3),
            // Adjacent, destination below source
            case(32, 8, 3),
            // Adjacent, unaligned addresses, destination above source
            case(3, 27, 3),
            // Adjacent, unaligned addresses, destination below source
            case(27, 3, 3),
            // Zero count, identical pointers
            case(8, 8, 0),
            // Zero count, destination inside the source range a non-zero count would cover
            case(4, 8, 0),
        ],
    );
}

/// Checks `ptr<felt>` copies, which go one field element at a time, between disjoint and adjacent
/// ranges in both directions, and with a zero count.
#[test]
fn mem_cpy_felt_elements() {
    check_table(
        CopyOp::MemCpy,
        Type::Felt,
        &felt_region(REGION_LEN),
        &[
            // Disjoint, destination above source
            case(0, 32, 6),
            // Disjoint, destination below source
            case(32, 0, 6),
            // Adjacent, destination above source
            case(4, 28, 6),
            // Adjacent, destination below source
            case(28, 4, 6),
            // Zero count, identical pointers
            case(8, 8, 0),
            // Zero count, destination inside the source range a non-zero count would cover
            case(4, 8, 0),
        ],
    );
}

/// Checks `ptr<u128>` copies, which go a word at a time and require 16-byte aligned addresses,
/// between disjoint and adjacent ranges in both directions, and with a zero count.
#[test]
fn mem_cpy_u128_elements() {
    check_table(
        CopyOp::MemCpy,
        Type::U128,
        &patterned_region(REGION_LEN),
        &[
            // Disjoint, destination above source
            case(0, 48, 1),
            // Disjoint, destination below source
            case(48, 0, 1),
            // Adjacent, destination above source
            case(0, 32, 2),
            // Adjacent, destination below source
            case(32, 0, 2),
            // Adjacent single values, destination above source
            case(16, 32, 1),
            // Adjacent single values, destination below source
            case(32, 16, 1),
            // Zero count, identical pointers
            case(16, 16, 0),
            // Zero count, destination inside the source range a non-zero count would cover
            case(0, 16, 0),
        ],
    );
}

/// Checks that 32-byte aggregates (`[u32; 8]`) are copied two words per value, several values per
/// copy, between disjoint and adjacent ranges in both directions, and with a zero count.
#[test]
fn mem_cpy_multiword_elements() {
    check_table(
        CopyOp::MemCpy,
        Type::from(ArrayType::new(Type::U32, 8)),
        &patterned_region(192),
        &[
            // Disjoint, destination above source
            case(0, 96, 2),
            // Disjoint, destination below source
            case(96, 0, 2),
            // Adjacent, destination above source
            case(0, 96, 3),
            // Adjacent, destination below source
            case(96, 0, 3),
            // Zero count, identical pointers
            case(32, 32, 0),
            // Zero count, destination inside the source range a non-zero count would cover
            case(32, 48, 0),
        ],
    );
}

/// Checks that overlapping byte copies on the byte loop, where an offset or the count is not a
/// multiple of 4, trap.
#[test]
fn mem_cpy_overlapping_bytes_unaligned_traps() {
    assert_overlap_traps(Type::U8, &overlapping_cases(3, 1, 1, 21));
}

/// Checks that overlapping byte copies on the element path, where both offsets and the count are
/// multiples of 4, trap.
#[test]
fn mem_cpy_overlapping_bytes_element_aligned_traps() {
    assert_overlap_traps(Type::U8, &overlapping_cases(0, 4, 1, 16));
}

/// Checks that overlapping `ptr<i1>` copies trap on both the byte loop and the element path.
#[test]
fn mem_cpy_overlapping_i1_traps() {
    let byte_loop = overlapping_cases(3, 1, 1, 21);
    let element_path = overlapping_cases(0, 4, 1, 16);
    assert_overlap_traps(Type::I1, &[byte_loop, element_path].concat());
}

/// Checks that overlapping `ptr<u16>` copies trap, both with the ranges whole values apart and,
/// at unaligned addresses, with the ranges one byte apart or sharing exactly one byte.
#[test]
fn mem_cpy_overlapping_u16_traps() {
    let whole_values = overlapping_cases(2, 2, 2, 10);
    let single_bytes = overlapping_cases(3, 1, 2, 10);
    assert_overlap_traps(Type::U16, &[whole_values, single_bytes].concat());
}

/// Checks that overlapping `ptr<u32>` copies trap, both with the ranges whole values apart and,
/// at unaligned addresses, with the ranges one byte apart or sharing exactly one byte.
#[test]
fn mem_cpy_overlapping_u32_traps() {
    let whole_values = overlapping_cases(4, 4, 4, 6);
    let single_bytes = overlapping_cases(3, 1, 4, 6);
    assert_overlap_traps(Type::U32, &[whole_values, single_bytes].concat());
}

/// Checks that overlapping `ptr<u64>` copies trap, both with the ranges whole values apart and,
/// at unaligned addresses, with the ranges one byte apart or sharing exactly one byte.
#[test]
fn mem_cpy_overlapping_u64_traps() {
    let whole_values = overlapping_cases(8, 8, 8, 4);
    let single_bytes = overlapping_cases(3, 1, 8, 3);
    assert_overlap_traps(Type::U64, &[whole_values, single_bytes].concat());
}

/// Checks that overlapping `ptr<felt>` copies trap.
#[test]
fn mem_cpy_overlapping_felt_traps() {
    assert_overlap_traps(Type::Felt, &overlapping_cases(4, 4, 4, 6));
}

/// Checks that overlapping `ptr<u128>` word copies trap.
#[test]
fn mem_cpy_overlapping_u128_traps() {
    assert_overlap_traps(Type::U128, &overlapping_cases(16, 16, 16, 3));
}

/// Checks that overlapping copies of 32-byte aggregates (`[u32; 8]`) trap, also when the ranges
/// are half a value apart.
#[test]
fn mem_cpy_overlapping_multiword_traps() {
    let whole_values = overlapping_cases(32, 32, 32, 3);
    let half_values = overlapping_cases(32, 16, 32, 3);
    assert_overlap_traps(
        Type::from(ArrayType::new(Type::U32, 8)),
        &[whole_values, half_values].concat(),
    );
}

/// Checks that a `ptr<u32>` copy whose byte length `count * 4` does not fit in `u32` traps with
/// the byte length overflow assertion.
#[test]
fn mem_cpy_byte_length_overflow_traps() {
    assert_copy_traps(
        CopyOp::MemCpy,
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
    assert_copy_traps(
        CopyOp::MemCpy,
        Type::U8,
        u32::MAX - 7,
        FIXED_BASE,
        16,
        "memcpy source range is out of bounds",
    );
    assert_copy_traps(
        CopyOp::MemCpy,
        Type::U8,
        FIXED_BASE,
        u32::MAX - 7,
        16,
        "memcpy destination range is out of bounds",
    );
}

/// Checks that a word copy (`ptr<u128>`) whose source or destination address is not 16-byte
/// aligned traps with the word-copy alignment assertion, also when `count` is zero.
#[test]
fn mem_cpy_word_copy_unaligned_traps() {
    const MESSAGE: &str = "expected a 16-byte-aligned byte pointer for the word-copy fast path";
    assert_copy_traps(CopyOp::MemCpy, Type::U128, FIXED_BASE + 4, FIXED_BASE + 32, 1, MESSAGE);
    assert_copy_traps(CopyOp::MemCpy, Type::U128, FIXED_BASE, FIXED_BASE + 4, 1, MESSAGE);
    assert_copy_traps(CopyOp::MemCpy, Type::U128, FIXED_BASE + 4, FIXED_BASE + 32, 0, MESSAGE);
}

/// Runs `case` against `package`, compiled for `hir.mem_cpy` over `ptr<elem>`: checks that it traps
/// with [OVERLAP_MESSAGE] if its ranges overlap, and that it matches `copy_within` otherwise. In
/// both branches the offsets of `case` are relative to the region start of [check_copy].
fn check_copy_or_overlap_trap(
    package: &std::sync::Arc<miden_mast_package::Package>,
    context: &std::rc::Rc<midenc_hir::Context>,
    elem: &Type,
    base: u32,
    region: &[u8],
    case: Case,
) -> Result<(), TestCaseError> {
    let elem_size = elem.size_in_bytes() as u32;
    if ranges_overlap(case, elem_size) {
        let (src, dst) = (region_addr(base, case.src_off), region_addr(base, case.dst_off));
        check_copy_traps(CopyOp::MemCpy, package, context, src, dst, case.count, OVERLAP_MESSAGE)
    } else {
        check_copy(CopyOp::MemCpy, package, context, elem, base, region, case)
    }
}

/// Checks random byte copies (aligned or not) against `copy_within`, or against the overlap trap
/// when the ranges overlap; about half of the cases have both offsets and the count a multiple
/// of 4.
#[test]
fn mem_cpy_bytes_match_copy_within_or_trap() {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_copy(CopyOp::MemCpy, Type::U8);

    let config = proptest::test_runner::Config::with_cases(64);
    let res = TestRunner::new(config).run(
        &(
            any::<[u8; REGION_LEN]>(),
            random_word_aligned_addr(),
            any::<bool>(),
            0u32..48,
            0u32..48,
            0u32..=16,
        ),
        move |(region, base, element_aligned, src_off, dst_off, count)| {
            let copy = if element_aligned {
                case(4 * (src_off / 4), 4 * (dst_off / 4), 4 * (count / 4))
            } else {
                case(src_off, dst_off, count)
            };
            check_copy_or_overlap_trap(&package, &context, &Type::U8, base, &region, copy)
        },
    );
    assert_proptest_passed(res);
}

/// Checks random copies of u16, u32, u64 and u128 values against `copy_within`, or against the
/// overlap trap when the ranges overlap; the u16, u32 and u64 cases use arbitrary byte offsets,
/// the u128 cases offsets that are multiples of 16.
#[test]
fn mem_cpy_values_match_copy_within_or_trap() {
    setup::enable_compiler_instrumentation();
    let compiled = [Type::U16, Type::U32, Type::U64, Type::U128].map(|elem| {
        let (package, context) = compile_copy(CopyOp::MemCpy, elem.clone());
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
            check_copy_or_overlap_trap(package, context, elem, base, &region, copy)
        },
    );
    assert_proptest_passed(res);
}
