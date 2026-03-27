use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::{HirOpBuilder, assertions};
use midenc_hir::{Builder, Immediate, PointerType, Report, SourceSpan, Type, ValueRef};

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
    if let Some(memarg) = memarg {
        if memarg.offset != 0 {
            let imm = builder.imm(Immediate::U32(memarg.offset as u32), span);
            full_addr_int = builder.add(addr_u32, imm, span)?;
        }
        // TODO(pauls): For now, asserting alignment helps us catch mistakes/bugs, but we should
        // probably make this something that can be disabled to avoid the overhead in release builds
        if memarg.align > 0 {
            // Generate alignment assertion - aligned addresses should always produce 0 here
            let imm = builder.imm(Immediate::U32(2u32.pow(memarg.align as u32)), span);
            let align_offset = builder.r#mod(full_addr_int, imm, span)?;
            builder.assertz_with_error(align_offset, assertions::ASSERT_FAILED_ALIGNMENT, span)?;
        }
    };
    builder.inttoptr(full_addr_int, Type::from(PointerType::new(ptr_ty.clone())), span)
}
