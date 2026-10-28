//! Stores into memory cells that still hold a felt at or above `2^32` (#1482).
//!
//! Such cells appear when Rust reuses a stack slot that previously held a `Word` or another felt
//! value for an integer. A store into such a cell must not trap, and must behave as if the cell
//! held only the low 32 bits of that felt.
//!
//! The tests build the HIR by hand rather than compiling a Rust reproducer, because the LLVM
//! slot-reuse shape from the issue depends on LLVM's frame layout, which is fragile across
//! toolchain versions and optimization levels.

use std::{rc::Rc, sync::Arc};

use miden_debug::ExecutionTrace;
use miden_mast_package::Package;
use midenc_hir::{ArrayType, Context, OpBuilder, dialects::builtin::FunctionBuilder};

use super::{
    copy::{CopyOp, FIXED_BASE, compile_copy},
    *,
};

/// The original contents of the three words of cells starting at [FIRST_CELL], all distinct felts
/// at or above `2^32`: four guard cells, the five cells starting at [FIXED_BASE] the stores write
/// to, and three more guard cells, so a stray write next to the range is noticed.
const CELLS: [u64; 12] = [
    0x2718_2818_2845_9045,
    0x1414_2135_6237_3095,
    0x1732_0508_0756_8877,
    0x0246_8ace_1357_9bdf,
    0xaabb_ccdd_1122_3344,
    0x9988_7766_5544_3322,
    0x1357_9bdf_2468_ace0,
    0xfedc_ba98_7654_3210,
    0x0123_4567_89ab_cdef,
    0x3141_5926_5358_9793,
    0x0577_2156_6490_1532,
    0x1618_0339_8874_9894,
];

/// The index in [CELLS] of the cell at [FIXED_BASE].
const BASE_CELL: usize = 4;

/// The element address of the first of [CELLS], a word-aligned guard cell before [FIXED_BASE].
const FIRST_CELL: u32 = FIXED_BASE / 4 - BASE_CELL as u32;

// The initializer writes whole words, so [CELLS] must start and end on a word boundary.
const _: () = assert!(FIRST_CELL.is_multiple_of(4) && CELLS.len().is_multiple_of(4));

/// The initializer writing [CELLS] to the cells starting at [FIRST_CELL].
fn felt_cells() -> [Initializer<'static>; 1] {
    [Initializer::MemoryFelts {
        addr: FIRST_CELL,
        felts: Cow::Owned(CELLS.map(Felt::new_unchecked).to_vec()),
    }]
}

/// The expected contents of all of [CELLS] after writing the low `bits` bits of the
/// little-endian `bytes` at byte offset `offset` from [FIXED_BASE]: cells covered by the write
/// hold the low 32 bits of the original felt with the written bits merged in, the other cells keep
/// the original felt.
fn expected_cells(offset: u32, bytes: &[u8], bits: usize) -> [Felt; CELLS.len()] {
    let mut memory = CELLS.map(|felt| felt as u32).map(u32::to_le_bytes).concat();
    let start = (BASE_CELL * 4 + offset as usize) * 8;
    let range = start..start + bits;
    for (i, position) in range.clone().enumerate() {
        let bit = (bytes[i / 8] >> (i % 8)) & 1;
        let byte = &mut memory[position / 8];
        *byte = (*byte & !(1 << (position % 8))) | (bit << (position % 8));
    }
    core::array::from_fn(|cell| {
        let cell_bits = cell * 32..cell * 32 + 32;
        if cell_bits.start < range.end && range.start < cell_bits.end {
            let low = u32::from_le_bytes(memory[cell * 4..cell * 4 + 4].try_into().unwrap());
            Felt::new_unchecked(low as u64)
        } else {
            Felt::new_unchecked(CELLS[cell])
        }
    })
}

/// Checks that every one of [CELLS], starting at [FIRST_CELL], holds the full felt in
/// `expected`.
fn assert_cells(trace: &ExecutionTrace, expected: [Felt; CELLS.len()], case: &str) {
    for (cell, expected) in expected.into_iter().enumerate() {
        let addr = FIRST_CELL + cell as u32;
        // A raw element read, since `read_from_rust_memory::<u32>` silently drops the high bits
        // of a cell, which would hide a cell that was not truncated.
        let observed = trace.read_memory_element(addr);
        assert_eq!(
            observed,
            Some(expected),
            "{case}: cell {cell} (element address {addr}, original {:#x})",
            CELLS[cell]
        );
    }
}

/// Runs `package` over the felt cells with `args`, checks the cells against `expected` and that
/// the program returns 1.
fn run_over_felt_cells(
    package: &Arc<Package>,
    context: &Rc<Context>,
    args: &[Felt],
    expected: [Felt; CELLS.len()],
    case: &str,
) {
    run_over_felt_cells_with(package, context, [], args, expected, 1, case);
}

/// Runs `package` over the felt cells and the `extra` initializers with `args`, checks the cells
/// against `expected` and that the program returns `output`.
fn run_over_felt_cells_with<'a>(
    package: &Arc<Package>,
    context: &Rc<Context>,
    extra: impl IntoIterator<Item = Initializer<'a>>,
    args: &[Felt],
    expected: [Felt; CELLS.len()],
    output: u32,
    case: &str,
) {
    let observed = eval_package::<u32, _, _>(
        package.clone(),
        felt_cells().into_iter().chain(extra),
        args,
        context.session(),
        |trace| {
            assert_cells(trace, expected, case);
            Ok(())
        },
    )
    .unwrap_or_else(|err| panic!("{case}: {err}"));
    assert_eq!(observed, output, "{case}");
}

/// Stores `value` through `ptr`, for sub-word integers loads it back and asserts it is intact, and
/// returns the constant 1 that [run_over_felt_cells] asserts.
fn store_and_check(builder: &mut FunctionBuilder<'_, OpBuilder>, ptr: ValueRef, value: ValueRef) {
    let span = SourceSpan::default();
    builder.store(ptr, value, span).unwrap();
    let ty = value.borrow().ty().clone();
    // Packed aggregates such as `[u8; 3]` have no scalar load to read them back with.
    if ty.is_integer() && ty.size_in_bytes() < 4 {
        let loaded = builder.load(ptr, span).unwrap();
        builder.assert_eq(loaded, value, span).unwrap();
    }
    let result = builder.u32(1, span);
    builder.ret(Some(result), span).unwrap();
}

/// Compiles a function that stores its `ty` argument through a byte pointer at [FIXED_BASE] plus
/// its `u32` offset argument.
fn compile_dynamic_store(ty: &Type) -> (Arc<Package>, Rc<Context>) {
    compile_test_module([Type::U32, ty.clone()], [Type::U32], |builder| {
        let (offset, value) = {
            let block = builder.current_block();
            let block = block.borrow();
            let args = block.arguments();
            (args[0] as ValueRef, args[1] as ValueRef)
        };
        let span = SourceSpan::default();
        let base = builder.u32(FIXED_BASE, span);
        let addr = builder.add(base, offset, span).unwrap();
        let ptr = builder.inttoptr(addr, Type::from(PointerType::new(ty.clone())), span).unwrap();
        store_and_check(builder, ptr, value);
    })
}

/// Stores each of `values` at each byte offset in `offsets` from [FIXED_BASE] over the felt cells,
/// with the address computed at runtime from an argument offset.
fn check_dynamic_addr<T>(offsets: core::ops::RangeInclusive<u32>, values: [T; 3])
where
    T: ToMidenRepr + Into<Immediate> + Copy + core::fmt::Debug,
{
    setup::enable_compiler_instrumentation();

    let ty = Into::<Immediate>::into(values[0]).ty();
    let (package, context) = compile_dynamic_store(&ty);

    for offset in offsets {
        for value in values {
            let mut args = vec![Felt::new_unchecked(offset as u64)];
            value.push_to_operand_stack(&mut args);
            let case = format!("{ty} {value:#x?} at dynamic offset {offset}");
            // An `i1` occupies a byte but writes only its low bit.
            let expected = expected_cells(offset, &value.to_bytes(), ty.size_in_bits());
            run_over_felt_cells(&package, &context, &args, expected, &case);
        }
    }
}

/// Stores `value` through an element-address-space pointer to the felt cell at [FIXED_BASE], and
/// loads it back.
///
/// `mask` covers the bits of the cell the value occupies, and `positioned` is the bit pattern of
/// the value at its position within the cell.
fn check_element_ptr(value: Immediate, mask: u32, positioned: u32) {
    setup::enable_compiler_instrumentation();

    let ty = value.ty();
    let (package, context) =
        compile_test_module([element_ptr(ty.clone())], [Type::U32], |builder| {
            let ptr = builder.current_block().borrow().arguments()[0] as ValueRef;
            let value = builder.imm(value, SourceSpan::default());
            store_and_check(builder, ptr, value);
        });

    let mut expected = CELLS.map(Felt::new_unchecked);
    expected[BASE_CELL] =
        Felt::new_unchecked(((CELLS[BASE_CELL] as u32 & !mask) | positioned) as u64);
    let case = format!("{ty} {value} through an element pointer");
    run_over_felt_cells(
        &package,
        &context,
        &[Felt::new_unchecked((FIRST_CELL + BASE_CELL as u32) as u64)],
        expected,
        &case,
    );
}

/// Stores each of `values`, the bits of a packed `ty` value, at byte offsets 0..=3 from
/// [FIXED_BASE] over the felt cells, with the address computed at runtime from an argument offset.
fn check_packed(ty: Type, values: [u32; 3]) {
    setup::enable_compiler_instrumentation();

    let (package, context) = compile_dynamic_store(&ty);

    for offset in 0..=3 {
        for value in values {
            let args = [Felt::new_unchecked(offset as u64), Felt::new_unchecked(value as u64)];
            let case = format!("{ty} {value:#x} at dynamic offset {offset}");
            let expected = expected_cells(offset, &value.to_le_bytes(), ty.size_in_bits());
            run_over_felt_cells(&package, &context, &args, expected, &case);
        }
    }
}

/// Fills the felt cells with `value` using `hir.mem_set` through a byte pointer, for each
/// `(offset, count)` in `cases`: `count` values starting at byte offset `offset` from
/// [FIXED_BASE].
fn check_mem_set<T>(value: T, cases: &[(u32, u32)])
where
    T: ToMidenRepr + Into<Immediate> + Copy + core::fmt::Debug,
{
    setup::enable_compiler_instrumentation();

    let ty = Into::<Immediate>::into(value).ty();
    let (package, context) =
        compile_test_module([Type::U32, Type::U32, ty.clone()], [Type::U32], |builder| {
            let (offset, count, value) = {
                let block = builder.current_block();
                let block = block.borrow();
                let args = block.arguments();
                (args[0] as ValueRef, args[1] as ValueRef, args[2] as ValueRef)
            };
            let span = SourceSpan::default();
            let base = builder.u32(FIXED_BASE, span);
            let addr = builder.add(base, offset, span).unwrap();
            let ptr =
                builder.inttoptr(addr, Type::from(PointerType::new(ty.clone())), span).unwrap();
            builder.memset(ptr, count, value, span).unwrap();
            let result = builder.u32(1, span);
            builder.ret(Some(result), span).unwrap();
        });

    for &(offset, count) in cases {
        let mut args = vec![Felt::new_unchecked(offset as u64), Felt::new_unchecked(count as u64)];
        value.push_to_operand_stack(&mut args);
        let case = format!("hir.mem_set of {count} x {ty} {value:#x?} at offset {offset}");
        let bytes = value.to_bytes().repeat(count as usize);
        let expected = expected_cells(offset, &bytes, bytes.len() * 8);
        run_over_felt_cells(&package, &context, &args, expected, &case);
    }
}

/// A `u8` store at a runtime byte address over felt cells merges into their low 32 bits.
#[test]
fn store_u8_over_felt_dynamic_addr() {
    check_dynamic_addr(0..=3, [0u8, u8::MAX, 0xa5]);
}

/// A `u16` store at a runtime byte address over felt cells merges into their low 32 bits.
#[test]
fn store_u16_over_felt_dynamic_addr() {
    check_dynamic_addr(0..=3, [0u16, u16::MAX, 0xa55a]);
}

/// An unaligned `u32` store at a runtime byte address over felt cells merges into their low 32
/// bits.
#[test]
fn store_u32_over_felt_dynamic_addr() {
    check_dynamic_addr(1..=3, [0u32, u32::MAX, 0xa55a_c33c]);
}

/// An unaligned `u64` store at a runtime byte address over felt cells merges into their low 32
/// bits.
#[test]
fn store_u64_over_felt_dynamic_addr() {
    check_dynamic_addr(1..=3, [0u64, u64::MAX, 0xa55a_c33c_0ff0_9669]);
}

/// An unaligned `u128` store at a runtime byte address over felt cells merges into their low 32
/// bits.
#[test]
fn store_u128_over_felt_dynamic_addr() {
    check_dynamic_addr(1..=3, [0u128, u128::MAX, 0xa55a_c33c_0ff0_9669_1234_5678_9abc_def0]);
}

/// An `i1` store at a runtime byte address over felt cells merges its bit into their low 32 bits.
#[test]
fn store_i1_over_felt_dynamic_addr() {
    check_dynamic_addr(0..=3, [false, true, true]);
}

/// A `u8` store through an element pointer to a felt cell merges into its low 32 bits.
#[test]
fn store_u8_over_felt_element_ptr() {
    check_element_ptr(Immediate::U8(0xa5), 0xff, 0xa5);
}

/// A `u16` store through an element pointer to a felt cell merges into its low 32 bits.
#[test]
fn store_u16_over_felt_element_ptr() {
    check_element_ptr(Immediate::U16(0xa55a), 0xffff, 0xa55a);
}

/// An `i1` store through an element pointer to a felt cell merges into its low 32 bits.
#[test]
fn store_i1_over_felt_element_ptr() {
    check_element_ptr(Immediate::I1(true), 1, 1);
}

/// A packed 24-bit `[u8; 3]` store at a runtime byte address over felt cells merges into their low
/// 32 bits, also when it crosses into the next cell.
#[test]
fn store_u8x3_over_felt_dynamic_addr() {
    check_packed(Type::from(ArrayType::new(Type::U8, 3)), [0, 0xff_ffff, 0xa5_5ac3]);
}

/// A packed 17-bit `[i1; 3]` store at a runtime byte address over felt cells merges into their low
/// 32 bits, also when it crosses into the next cell.
#[test]
fn store_i1x3_over_felt_dynamic_addr() {
    check_packed(Type::from(ArrayType::new(Type::I1, 3)), [0, 0x1_ffff, 0x1_0001]);
}

/// A packed 25-bit `[i1; 4]` store at a runtime byte address over felt cells merges into their low
/// 32 bits, also when it crosses into the next cell, which it does already at byte offset 1.
#[test]
fn store_i1x4_over_felt_dynamic_addr() {
    check_packed(Type::from(ArrayType::new(Type::I1, 4)), [0, 0x1ff_ffff, 0x100_0001]);
}

/// A `hir.mem_set` of `u8` values over felt cells merges into the low 32 bits of every cell it
/// touches, within one cell and across cells.
#[test]
fn mem_set_u8_over_felt() {
    check_mem_set(0xa5u8, &[(1, 2), (0, 4), (2, 5), (3, 6)]);
}

/// A `hir.mem_set` of `u16` values over felt cells merges into the low 32 bits of every cell it
/// touches, within one cell and across cells.
#[test]
fn mem_set_u16_over_felt() {
    check_mem_set(0xa55au16, &[(1, 1), (0, 2), (1, 3), (3, 4)]);
}

/// An unaligned byte `hir.mem_move` or `hir.mem_cpy` into felt cells merges the copied bytes into
/// the low 32 bits of every destination cell it touches, within one cell and across cells.
///
/// The source is a separate region of ordinary `u32` cells, since loads from a felt cell trap.
#[test]
fn mem_copy_bytes_over_felt() {
    setup::enable_compiler_instrumentation();

    let src = FIXED_BASE + 64;
    let src_bytes = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc];
    for op in [CopyOp::MemMove, CopyOp::MemCpy] {
        let (package, context) = compile_copy(op, Type::U8);
        for (dst_offset, count) in [(1, 6), (3, 6), (2, 1), (5, 9)] {
            let case = format!("{op:?} of {count} bytes to offset {dst_offset}");
            let expected = expected_cells(dst_offset, &src_bytes, count as usize * 8);
            let source = [Initializer::MemoryBytes {
                addr: src,
                bytes: &src_bytes,
            }];
            let args = [src, FIXED_BASE + dst_offset, count].map(Felt::from);
            run_over_felt_cells_with(&package, &context, source, &args, expected, count, &case);
        }
    }
}

/// The shape of #1482: a felt stored in the program, then a `u16` stored at the same byte address.
#[test]
fn store_u16_after_felt_store_in_program() {
    setup::enable_compiler_instrumentation();

    // A felt the initializer does not write, so a dropped in-program felt store is noticed.
    let felt = (1u64 << 40) + 0xdead_0000;
    let (package, context) = compile_test_module([Type::U16], [Type::U32], |builder| {
        let value = builder.current_block().borrow().arguments()[0] as ValueRef;
        let span = SourceSpan::default();
        let addr = builder.u32(FIXED_BASE, span);
        let felt_ptr =
            builder.inttoptr(addr, Type::from(PointerType::new(Type::Felt)), span).unwrap();
        let felt = builder.felt(Felt::new_unchecked(felt), span);
        builder.store(felt_ptr, felt, span).unwrap();
        let u16_ptr =
            builder.inttoptr(addr, Type::from(PointerType::new(Type::U16)), span).unwrap();
        store_and_check(builder, u16_ptr, value);
    });

    let mut expected = CELLS.map(Felt::new_unchecked);
    expected[BASE_CELL] = Felt::new_unchecked(0xdead_beef);
    let case = "u16 store after a felt store in the program";
    run_over_felt_cells(&package, &context, &[Felt::from(0xbeef_u16)], expected, case);
}
