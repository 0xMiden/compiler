//! Shared helpers of the `hir.mem_cpy` and `hir.mem_move` tests: test module compilation, region
//! contents, and checks that compare a copy with `copy_within` or assert that it traps.

use std::{rc::Rc, sync::Arc};

use super::*;
use crate::trap_helpers::{panic_message, trap_matches};

/// The default size in bytes of the memory region a test case initializes and checks.
pub(super) const REGION_LEN: usize = 64;

/// The size in bytes of each guard band placed directly below and above the region; a multiple of
/// 16, so the region keeps the alignment of the buffer.
const GUARD: usize = 32;

/// The value of every guard band byte; it does not occur in the first 206 bytes of
/// [patterned_region] or [felt_region].
const GUARD_BYTE: u8 = 0xa5;

/// A 16-byte aligned byte address above the pages reserved for the Rust stack.
pub(super) const FIXED_BASE: u32 = 17 * 2u32.pow(16);

/// The copy operation a test exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CopyOp {
    /// `hir.mem_cpy`, whose source and destination ranges must be disjoint.
    MemCpy,
    /// `hir.mem_move`, whose source and destination ranges may overlap.
    MemMove,
}

impl CopyOp {
    /// The name of the operation in the IR.
    const fn name(self) -> &'static str {
        match self {
            Self::MemCpy => "hir.mem_cpy",
            Self::MemMove => "hir.mem_move",
        }
    }
}

/// A single copy case: the offsets are in bytes from the region base, `count` is in units of the
/// pointee type.
#[derive(Debug, Clone, Copy)]
pub(super) struct Case {
    pub src_off: u32,
    pub dst_off: u32,
    pub count: u32,
}

/// Shorthand constructor for a [Case].
pub(super) const fn case(src_off: u32, dst_off: u32, count: u32) -> Case {
    Case {
        src_off,
        dst_off,
        count,
    }
}

/// Returns `len` bytes of non-trivial region contents for the table-driven tests.
pub(super) fn patterned_region(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(7).wrapping_add(3)).collect()
}

/// Returns [patterned_region] with the top bit of every 4th byte cleared, so that every 4-byte
/// group, read as a little-endian `u32`, is a valid field element below 2^31.
pub(super) fn felt_region(len: usize) -> Vec<u8> {
    let mut region = patterned_region(len);
    for byte in region.iter_mut().skip(3).step_by(4) {
        *byte &= 0x7f;
    }
    region
}

/// Reads `len` bytes of Rust memory starting at the element-aligned byte address `addr`, failing
/// if an element holds a value that does not fit in a `u32`.
pub(super) fn read_rust_bytes(
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
/// `op` on `src`, `dst` and `count` and returns `count`.
pub(super) fn compile_copy(
    op: CopyOp,
    elem: Type,
) -> (Arc<miden_mast_package::Package>, Rc<midenc_hir::Context>) {
    let ptr_ty = Type::from(PointerType::new(elem));
    compile_test_module([ptr_ty.clone(), ptr_ty, Type::U32], [Type::U32], |builder| {
        let block = builder.current_block();
        let (src, dst, count) = {
            let block_ref = block.borrow();
            let args = block_ref.arguments();
            (args[0] as ValueRef, args[1] as ValueRef, args[2] as ValueRef)
        };
        match op {
            CopyOp::MemCpy => {
                builder.memcpy(src, dst, count, SourceSpan::default()).unwrap();
            }
            CopyOp::MemMove => {
                builder.memmove(src, dst, count, SourceSpan::default()).unwrap();
            }
        }
        builder.ret(Some(count), SourceSpan::default()).unwrap();
    })
}

/// Runs `case` against `package`, compiled for `op`, on a region initialized with `region`, and
/// checks that the region afterwards equals the result of `copy_within` over the same bytes while
/// the guard bands around it are left untouched.
///
/// The buffer starts at `base` with a [GUARD]-byte band of [GUARD_BYTE], followed by the region and
/// another such band; the offsets of `case` are relative to the region start `base + GUARD`.
/// `elem` is the pointee type `package` was compiled for; it gives the size of one unit of
/// `case.count`.
pub(super) fn check_copy(
    op: CopyOp,
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
                "unexpected memory contents (region or guard bands) after {} of {} with \
                 src_off={}, dst_off={}, count={}, base={}",
                op.name(),
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

/// Runs every case of a table-driven test of `op` on the fixed base address with `region` as the
/// initial region contents.
pub(super) fn check_table(op: CopyOp, elem: Type, region: &[u8], cases: &[Case]) {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_copy(op, elem.clone());
    for &case in cases {
        if let Err(err) = check_copy(op, &package, &context, &elem, FIXED_BASE, region, case) {
            panic!("FAILURE for {case:?}: {err}");
        }
    }
}

/// Runs `package`, compiled for `op`, with the raw byte addresses `src` and `dst` and the given
/// `count`, and checks that it traps with the assertion `message`, see [trap_matches].
pub(super) fn check_copy_traps(
    op: CopyOp,
    package: &Arc<miden_mast_package::Package>,
    context: &Rc<midenc_hir::Context>,
    src: u32,
    dst: u32,
    count: u32,
    message: &str,
) -> Result<(), TestCaseError> {
    let args = [
        Felt::new_unchecked(src as u64),
        Felt::new_unchecked(dst as u64),
        Felt::new_unchecked(count as u64),
    ];
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eval_package::<u32, _, _>(package.clone(), None, &args, context.session(), |_| Ok(()))
    }));
    let err = match result {
        Err(panic) => panic_message(panic),
        Ok(output) => {
            return Err(TestCaseError::fail(format!(
                "expected {} to trap with {message:?} for src={src}, dst={dst}, count={count}, \
                 got {output:?}",
                op.name()
            )));
        }
    };
    if trap_matches(&err, message) {
        Ok(())
    } else {
        Err(TestCaseError::fail(format!(
            "expected a trap containing {message:?} for src={src}, dst={dst}, count={count}, got: \
             {err}"
        )))
    }
}

/// Asserts that `op` over `ptr<elem>` with the raw byte addresses `src` and `dst` and the given
/// `count` traps with the assertion `message`, see [trap_matches].
pub(super) fn assert_copy_traps(
    op: CopyOp,
    elem: Type,
    src: u32,
    dst: u32,
    count: u32,
    message: &str,
) {
    setup::enable_compiler_instrumentation();
    let (package, context) = compile_copy(op, elem);
    if let Err(err) = check_copy_traps(op, &package, &context, src, dst, count, message) {
        panic!("FAILURE: {err}");
    }
}

/// Panics with the minimal failing case if a property test run failed.
pub(super) fn assert_proptest_passed<T: std::fmt::Debug>(res: Result<(), TestError<T>>) {
    match res {
        Err(TestError::Fail(reason, value)) => {
            panic!("FAILURE: {}\nMinimal failing case: {value:?}", reason.message());
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {res:?}"),
    }
}
