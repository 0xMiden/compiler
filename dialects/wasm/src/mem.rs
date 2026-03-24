use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::{HirOpBuilder, assertions};
use midenc_hir::{Builder, Immediate, PointerType, Report, SourceSpan, Type, ValueRef};
use wasmparser::MemArg;

// TODO compare with old `fn prepare_addr`
pub fn prepare_addr<'a, B: ?Sized + Builder>(
    addr_int: ValueRef,
    ptr_ty: &Type,
    memarg: Option<&MemArg>,
    builder: &mut (impl ArithOpBuilder<'a, B> + HirOpBuilder<'a, B>),
    span: SourceSpan,
) -> Result<ValueRef, Report> {
    let addr_int_ty = addr_int.borrow().ty().clone();
    let addr_u32 = if addr_int_ty == Type::U32 {
        addr_int
    } else if addr_int_ty == Type::I32 {
        builder.bitcast(addr_int, Type::U32, span)?
    } else if matches!(addr_int_ty, Type::Ptr(_)) {
        builder.ptrtoint(addr_int, Type::U32, span)?
    } else {
        panic!("unexpected type used as pointer value: {addr_int_ty}");
    };
    let mut full_addr_int = addr_u32;
    if let Some(memarg) = memarg {
        if memarg.offset != 0 {
            let imm = builder.imm(Immediate::U32(memarg.offset as u32), span);
            full_addr_int = builder.add(addr_u32, imm, span)?;
        }
        if memarg.align > 0 {
            let imm = builder.imm(Immediate::U32(2u32.pow(memarg.align as u32)), span);
            let align_offset = builder.r#mod(full_addr_int, imm, span)?;
            builder.assertz_with_error(align_offset, assertions::ASSERT_FAILED_ALIGNMENT, span)?;
        }
    };
    builder.inttoptr(full_addr_int, Type::from(PointerType::new(ptr_ty.clone())), span)
}
