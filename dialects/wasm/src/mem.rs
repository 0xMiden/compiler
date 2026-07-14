use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::{HirOpBuilder, assertions};
use midenc_hir::{
    AddressSpace, Builder, Immediate, PointerType, Report, SourceSpan, Type, ValueRef,
};

/// The base-2 logarithm of the byte alignment of a Miden memory element.
const ELEMENT_ALIGNMENT_LOG2: u8 = 2;

/// The size of a Miden memory element in the byte-addressable address space.
const ELEMENT_SIZE_BYTES: u32 = 1 << ELEMENT_ALIGNMENT_LOG2;

/// Represents a memory immediate in a WebAssembly memory instruction.
///
/// Mirrors `MemArg` from the `wasmparser` crate.
#[derive(Debug, Clone, Copy, Default, Hash, Eq, PartialEq)]
pub struct WasmMemArg {
    /// A fixed byte-offset that this memory immediate specifies.
    ///
    /// Note that the memory64 proposal can specify a full 64-bit byte offset while otherwise
    /// only 32-bit offsets are allowed. Once validated memory immediates for 32-bit memories are
    /// guaranteed to be at most `u32::MAX` whereas 64-bit memories can use the full 64-bits.
    pub offset: u64,
    /// Alignment, stored as `n` where the actual alignment is `2^n`
    pub align: u8,
}

impl WasmMemArg {
    pub const fn new(offset: u64, align: u8) -> Self {
        Self { offset, align }
    }
}

pub trait WasmMemOpBuilder<'a, B: ?Sized + Builder>:
    ArithOpBuilder<'a, B> + HirOpBuilder<'a, B>
{
}

impl<'a, B, T> WasmMemOpBuilder<'a, B> for T
where
    B: ?Sized + Builder,
    T: ?Sized + ArithOpBuilder<'a, B> + HirOpBuilder<'a, B>,
{
}

/// Prepares `addr_int` to be used as pointer to a value of type `ptr_ty`.
///
/// The address space of the returned pointer depends on the access:
///
/// - An access which promises element alignment (`memarg.align >= 2`) of an element-sized pointee
///   ([Type::Felt], [Type::I32], [Type::U32]) produces an [AddressSpace::Element] pointer holding
///   the element address (the byte address divided by the element size), which lowers to native
///   Miden memory operations.
/// - Any other access produces an [AddressSpace::Byte] pointer holding the byte address.
///
/// Callers must therefore not treat the returned pointer value as a byte address (e.g. for
/// pointer arithmetic).
///
/// Alignment promises are enforced: when `memarg.align > 0`, an access whose effective address
/// violates the promised alignment traps with [assertions::ASSERT_FAILED_ALIGNMENT]. This is a
/// deliberate deviation from Wasm semantics, where the alignment immediate is only a hint that
/// must not affect the result of an access. It is sound for Rust/LLVM-produced modules, where
/// violating a promised alignment is undefined behavior in the source program, and it keeps
/// aligned accesses cheap, which matters far more on this target than supporting modules that
/// overpromise alignment.
///
/// # Panics
///
/// Panics if `addr_int` does not have type `I32`.
pub fn prepare_addr<'a, B: ?Sized + Builder>(
    addr_int: ValueRef,
    ptr_ty: &Type,
    memarg: Option<WasmMemArg>,
    builder: &mut (impl WasmMemOpBuilder<'a, B> + ?Sized),
    span: SourceSpan,
) -> Result<ValueRef, Report> {
    let addr_int_ty = addr_int.borrow().ty().clone();
    assert!(
        matches!(addr_int_ty, Type::I32),
        "pointer address must have type I32, got {addr_int_ty}"
    );
    let addr_u32 = builder.bitcast(addr_int, Type::U32, span)?;
    let mut full_addr_int = addr_u32;
    let mut address_space = AddressSpace::Byte;
    if let Some(memarg) = memarg {
        if memarg.offset != 0 {
            let imm = builder.imm(Immediate::U32(memarg.offset as u32), span);
            full_addr_int = builder.add(addr_u32, imm, span)?;
        }
        if memarg.align > 0 {
            let (addr, addrspace) =
                enforce_alignment(full_addr_int, ptr_ty, memarg.align, builder, span)?;
            full_addr_int = addr;
            address_space = addrspace;
        }
    };
    builder.inttoptr(
        full_addr_int,
        Type::from(PointerType::new_with_address_space(ptr_ty.clone(), address_space)),
        span,
    )
}

/// Emits a runtime check that `addr` satisfies the alignment promised by `align` (given as a
/// base-2 logarithm), and selects the address space of the resulting pointer.
///
/// Element-aligned accesses of element-sized pointees are converted to the element address space:
/// a single `divmod` yields both the element address and the proof of alignment, making the check
/// nearly free. The alignment assertion in that branch is load-bearing, not a debugging aid: the
/// quotient is used as the address, so without the assertion a misaligned access would silently
/// target the containing element instead of trapping.
fn enforce_alignment<'a, B: ?Sized + Builder>(
    addr: ValueRef,
    ptr_ty: &Type,
    align: u8,
    builder: &mut (impl WasmMemOpBuilder<'a, B> + ?Sized),
    span: SourceSpan,
) -> Result<(ValueRef, AddressSpace), Report> {
    if align >= ELEMENT_ALIGNMENT_LOG2 && matches!(ptr_ty, Type::Felt | Type::I32 | Type::U32) {
        // A successful divmod both proves natural alignment and converts the byte address
        // to the element address consumed directly by Miden memory operations.
        let element_size = builder.imm(Immediate::U32(ELEMENT_SIZE_BYTES), span);
        let (element_addr, byte_offset) = builder.divmod(addr, element_size, span)?;
        builder.assertz_with_error(byte_offset, assertions::ASSERT_FAILED_ALIGNMENT, span)?;
        Ok((element_addr, AddressSpace::Element))
    } else {
        // Generate alignment assertion - aligned addresses should always produce 0 here
        //
        // TODO(pauls): For now, asserting alignment helps us catch mistakes/bugs, but we should
        // probably make this something that can be disabled to avoid the overhead in release
        // builds. Note that this only applies to this byte-address-space branch; the element
        // branch above requires its assertion for correctness.
        let imm = builder.imm(Immediate::U32(2u32.pow(align as u32)), span);
        let align_offset = builder.r#mod(addr, imm, span)?;
        builder.assertz_with_error(align_offset, assertions::ASSERT_FAILED_ALIGNMENT, span)?;
        Ok((addr, AddressSpace::Byte))
    }
}
