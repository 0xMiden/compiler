//! Stores into memory cells that still hold a felt at or above `2^32` (#1482).
//!
//! Such cells appear when Rust reuses a stack slot that previously held a `Word` or another felt
//! value for an integer. A store into such a cell must not trap, and must behave as if the cell
//! held only the low 32 bits of that felt.

use std::{rc::Rc, sync::Arc};

use miden_debug::ExecutionTrace;
use miden_mast_package::Package;
use midenc_hir::{AddressSpace, Context, OpBuilder, dialects::builtin::FunctionBuilder};

use super::*;

/// The byte address of the first of the five pre-initialized cells.
const BASE: u32 = 17 * 2u32.pow(16);

/// The original contents of the five cells starting at [BASE], all above `2^32`.
const CELLS: [u64; 5] = [
    0xaabb_ccdd_1122_3344,
    0x9988_7766_5544_3322,
    0x1357_9bdf_2468_ace0,
    0xfedc_ba98_7654_3210,
    0x0123_4567_89ab_cdef,
];

/// Integer types stored over a felt cell through a byte pointer.
trait Stored: ToMidenRepr + Copy + core::fmt::Debug {
    /// The HIR type of the stored value.
    fn hir_type() -> Type;

    /// The little-endian bytes of the value.
    fn le_bytes(self) -> Vec<u8>;
}

macro_rules! impl_stored {
    ($($rust_ty:ty => $hir_ty:ident),* $(,)?) => {
        $(
            impl Stored for $rust_ty {
                fn hir_type() -> Type {
                    Type::$hir_ty
                }

                fn le_bytes(self) -> Vec<u8> {
                    self.to_le_bytes().to_vec()
                }
            }
        )*
    };
}

impl_stored!(u8 => U8, u16 => U16, u32 => U32, u64 => U64, u128 => U128);

/// The initializer writing [CELLS] to the five cells starting at [BASE].
fn felt_cells() -> [Initializer<'static>; 1] {
    [Initializer::MemoryFelts {
        addr: BASE / 4,
        felts: Cow::Owned(CELLS.map(Felt::new_unchecked).to_vec()),
    }]
}

/// The expected contents of the five cells after storing `bytes` at byte offset `offset` from
/// [BASE]: cells covered by the store hold the low 32 bits of the original felt with the stored
/// bytes merged in, the other cells keep the original felt.
fn expected_cells(offset: u32, bytes: &[u8]) -> [Felt; 5] {
    let mut memory = CELLS.map(|felt| felt as u32).map(u32::to_le_bytes).concat();
    let range = offset as usize..offset as usize + bytes.len();
    memory[range.clone()].copy_from_slice(bytes);
    core::array::from_fn(|cell| {
        let cell_bytes = cell * 4..cell * 4 + 4;
        if cell_bytes.start < range.end && range.start < cell_bytes.end {
            let low = u32::from_le_bytes(memory[cell_bytes].try_into().unwrap());
            Felt::new_unchecked(low as u64)
        } else {
            Felt::new_unchecked(CELLS[cell])
        }
    })
}

/// Reads the five cells starting at [BASE] as raw felts and compares them with `expected`.
///
/// Raw element reads are used because `read_from_rust_memory::<u32>` silently drops the high
/// bits of a cell, which would hide a cell that was not truncated.
fn assert_cells(trace: &ExecutionTrace, expected: [Felt; 5], case: &str) {
    for (cell, expected) in expected.into_iter().enumerate() {
        let addr = BASE / 4 + cell as u32;
        let observed = trace.read_memory_element(addr);
        assert_eq!(
            observed,
            Some(expected),
            "{case}: cell {cell} (element address {addr}, original {:#x})",
            CELLS[cell]
        );
    }
}

/// Runs `package` over the felt cells with `args` and checks the cells against `expected`.
fn run_over_felt_cells(
    package: &Arc<Package>,
    context: &Rc<Context>,
    args: &[Felt],
    expected: [Felt; 5],
    case: &str,
) {
    let output = eval_package::<u32, _, _>(
        package.clone(),
        felt_cells(),
        args,
        context.session(),
        |trace| {
            assert_cells(trace, expected, case);
            Ok(())
        },
    )
    .unwrap_or_else(|err| panic!("{case}: {err}"));
    assert_eq!(output, 1, "{case}");
}

/// Stores `value` through `ptr`, and for sub-word values loads it back and asserts it is intact.
fn store_and_check(builder: &mut FunctionBuilder<'_, OpBuilder>, ptr: ValueRef, value: ValueRef) {
    let span = SourceSpan::default();
    builder.store(ptr, value, span).unwrap();
    if value.borrow().ty().size_in_bytes() < 4 {
        let loaded = builder.load(ptr, span).unwrap();
        builder.assert_eq(loaded, value, span).unwrap();
    }
    let result = builder.u32(1, span);
    builder.ret(Some(result), span).unwrap();
}

/// Stores each of `values` at each byte offset in `offsets` from [BASE] over the felt cells, with
/// the address computed at runtime from an argument offset.
fn check_dynamic_addr<T: Stored>(offsets: core::ops::RangeInclusive<u32>, values: [T; 3]) {
    setup::enable_compiler_instrumentation();

    let (package, context) =
        compile_test_module([Type::U32, T::hir_type()], [Type::U32], |builder| {
            let (offset, value) = {
                let block = builder.current_block();
                let block = block.borrow();
                let args = block.arguments();
                (args[0] as ValueRef, args[1] as ValueRef)
            };
            let span = SourceSpan::default();
            let base = builder.u32(BASE, span);
            let addr = builder.add(base, offset, span).unwrap();
            let ptr = builder
                .inttoptr(addr, Type::from(PointerType::new(T::hir_type())), span)
                .unwrap();
            store_and_check(builder, ptr, value);
        });

    for offset in offsets {
        for value in values {
            let mut args = vec![Felt::new_unchecked(offset as u64)];
            value.push_to_operand_stack(&mut args);
            let case = format!("{} {value:#x?} at dynamic offset {offset}", T::hir_type());
            let expected = expected_cells(offset, &value.le_bytes());
            run_over_felt_cells(&package, &context, &args, expected, &case);
        }
    }
}

/// Stores each of `values` at each byte offset in `offsets` from [BASE] over the felt cells, with
/// the address a compile-time constant.
fn check_constant_addr<T: Stored>(offsets: core::ops::RangeInclusive<u32>, values: [T; 3]) {
    setup::enable_compiler_instrumentation();

    for offset in offsets {
        let (package, context) = compile_test_module([T::hir_type()], [Type::U32], |builder| {
            let value = builder.current_block().borrow().arguments()[0] as ValueRef;
            let span = SourceSpan::default();
            let addr = builder.u32(BASE + offset, span);
            let ptr = builder
                .inttoptr(addr, Type::from(PointerType::new(T::hir_type())), span)
                .unwrap();
            store_and_check(builder, ptr, value);
        });

        for value in values {
            let mut args = Vec::new();
            value.push_to_operand_stack(&mut args);
            let case = format!("{} {value:#x?} at constant offset {offset}", T::hir_type());
            let expected = expected_cells(offset, &value.le_bytes());
            run_over_felt_cells(&package, &context, &args, expected, &case);
        }
    }
}

/// Stores `value` through an element-address-space pointer to the first felt cell, and loads it
/// back; `mask` covers the bits of the cell the value occupies.
fn check_element_ptr(value: Immediate, mask: u32, bits: u32) {
    setup::enable_compiler_instrumentation();

    let ty = value.ty();
    let ptr_ty = Type::from(PointerType::new_with_address_space(ty.clone(), AddressSpace::Element));
    let (package, context) = compile_test_module([ptr_ty], [Type::U32], |builder| {
        let ptr = builder.current_block().borrow().arguments()[0] as ValueRef;
        let value = builder.imm(value, SourceSpan::default());
        store_and_check(builder, ptr, value);
    });

    let mut expected = CELLS.map(Felt::new_unchecked);
    expected[0] = Felt::new_unchecked(((CELLS[0] as u32 & !mask) | bits) as u64);
    let case = format!("{ty} {value} through an element pointer");
    run_over_felt_cells(
        &package,
        &context,
        &[Felt::new_unchecked((BASE / 4) as u64)],
        expected,
        &case,
    );
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

/// A `u8` store at a constant byte address over felt cells merges into their low 32 bits.
#[test]
fn store_u8_over_felt_constant_addr() {
    check_constant_addr(0..=3, [0u8, u8::MAX, 0xa5]);
}

/// A `u16` store at a constant byte address over felt cells merges into their low 32 bits.
#[test]
fn store_u16_over_felt_constant_addr() {
    check_constant_addr(0..=3, [0u16, u16::MAX, 0xa55a]);
}

/// An unaligned `u32` store at a constant byte address over felt cells merges into their low 32
/// bits.
#[test]
fn store_u32_over_felt_constant_addr() {
    check_constant_addr(1..=3, [0u32, u32::MAX, 0xa55a_c33c]);
}

/// An unaligned `u64` store at a constant byte address over felt cells merges into their low 32
/// bits.
#[test]
fn store_u64_over_felt_constant_addr() {
    check_constant_addr(1..=3, [0u64, u64::MAX, 0xa55a_c33c_0ff0_9669]);
}

/// An unaligned `u128` store at a constant byte address over felt cells merges into their low 32
/// bits.
#[test]
fn store_u128_over_felt_constant_addr() {
    check_constant_addr(1..=3, [0u128, u128::MAX, 0xa55a_c33c_0ff0_9669_1234_5678_9abc_def0]);
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

/// The shape of #1482: a felt stored in the program, then a `u16` stored at the same byte address.
#[test]
fn store_u16_after_felt_store_in_program() {
    setup::enable_compiler_instrumentation();

    let felt = 0xaabb_ccdd_1122_3344u64;
    let (package, context) = compile_test_module([Type::U16], [Type::U32], |builder| {
        let value = builder.current_block().borrow().arguments()[0] as ValueRef;
        let span = SourceSpan::default();
        let addr = builder.u32(BASE, span);
        let felt_ptr =
            builder.inttoptr(addr, Type::from(PointerType::new(Type::Felt)), span).unwrap();
        let felt = builder.felt(Felt::new_unchecked(felt), span);
        builder.store(felt_ptr, felt, span).unwrap();
        let u16_ptr =
            builder.inttoptr(addr, Type::from(PointerType::new(Type::U16)), span).unwrap();
        store_and_check(builder, u16_ptr, value);
    });

    let value = 0xbeefu16;
    let expected = Felt::new_unchecked(((felt as u32 & !0xffff) | value as u32) as u64);
    let output = eval_package::<u32, _, _>(
        package,
        [],
        &[Felt::new_unchecked(value as u64)],
        context.session(),
        |trace| {
            assert_eq!(trace.read_memory_element(BASE / 4), Some(expected));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, 1);
}
