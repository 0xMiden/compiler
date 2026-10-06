use midenc_hir::{AddressSpace, BuilderExt, Felt};

use super::*;

#[test]
fn mem_move_preserves_adjacent_full_width_felts() -> Result<(), Report> {
    let values = [1u64 << 32, (1u64 << 40) + 7, (1u64 << 48) + 9, 11]
        .map(|value| Immediate::Felt(Felt::new_unchecked(value)));
    let observed = eval_copy(CopyOp::MemMove, "copy_full_felts", values, 0, 1, 3)?;
    assert_eq!(observed, [values[0], values[0], values[1], values[2]].map(Value::Immediate));
    Ok(())
}

#[test]
fn element_store_aliases_byte_load() -> Result<(), Report> {
    let mut test = EvalTest::named("element_store_aliases_byte_load");
    test.with_function(&[], &[Type::U32]);
    {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        let native_addr = builder.u32(16, span);
        let native_ptr = builder.inttoptr(
            native_addr,
            Type::from(PointerType::new_with_address_space(Type::U32, AddressSpace::Element)),
            span,
        )?;
        let value = builder.u32(0x1234_5678, span);
        builder.store(native_ptr, value, span)?;
        let byte_addr = builder.u32(64, span);
        let byte_ptr =
            builder.inttoptr(byte_addr, Type::from(PointerType::new(Type::U32)), span)?;
        let loaded = builder.load(byte_ptr, span)?;
        builder.ret(Some(loaded), span)?;
    }
    let callable = test.function().borrow();
    let result = test.evaluator.eval_callable(&*callable, [])?;
    assert_eq!(result.as_slice(), &[Value::Immediate(Immediate::U32(0x1234_5678))]);
    Ok(())
}

#[test]
fn integer_memory_is_little_endian() -> Result<(), Report> {
    let mut test = EvalTest::default();
    test.evaluator.write_memory(64, Immediate::U64(0x0123_4567_89ab_cdef))?;
    assert_eq!(
        test.evaluator.read_memory_bytes(64, 8)?,
        [0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01]
    );
    Ok(())
}

#[test]
fn byte_access_rejects_high_field_elements() -> Result<(), Report> {
    let mut test = EvalTest::default();
    test.evaluator
        .write_memory(64, Immediate::Felt(Felt::new_unchecked(1u64 << 32)))?;
    assert!(test.evaluator.read_memory_bytes(64, 1).is_err());
    assert!(test.evaluator.write_memory_bytes(65, &[1]).is_err());
    Ok(())
}

#[test]
fn boolean_stores_preserve_neighboring_bits() -> Result<(), Report> {
    let mut test = EvalTest::default();
    test.evaluator.write_memory(64, Immediate::U32(0xaabb_fffe))?;
    test.evaluator.write_memory(64, Immediate::I1(true))?;
    assert_eq!(
        test.evaluator.read_memory(64, &Type::U32)?,
        Value::Immediate(Immediate::U32(0xaabb_ffff))
    );
    test.evaluator.write_memory(65, Immediate::I1(false))?;
    assert_eq!(
        test.evaluator.read_memory(64, &Type::U32)?,
        Value::Immediate(Immediate::U32(0xaabb_feff))
    );
    Ok(())
}

#[test]
fn memset_advances_by_pointee_size() -> Result<(), Report> {
    let mut test = EvalTest::named("memset_advances_by_pointee_size");
    test.with_function(&[], &[]);
    {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        let addr = builder.u32(64, span);
        let ptr = builder.inttoptr(addr, Type::from(PointerType::new(Type::U32)), span)?;
        let value = builder.u32(0x1234_5678, span);
        let count = builder.u32(3, span);
        builder.memset(ptr, count, value, span)?;
        builder.ret(None, span)?;
    }
    let callable = test.function().borrow();
    test.evaluator.eval_callable(&*callable, [])?;
    for addr in [64, 68, 72] {
        assert_eq!(
            test.evaluator.read_memory(addr, &Type::U32)?,
            Value::Immediate(Immediate::U32(0x1234_5678))
        );
    }
    Ok(())
}

#[test]
fn zero_length_word_copy_checks_alignment() -> Result<(), Report> {
    let result =
        eval_copy_at(CopyOp::MemMove, "zero_word_alignment", [Immediate::U128(0); 2], 65, 80, 0);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn field_element_access_checks_alignment() -> Result<(), Report> {
    let mut test = EvalTest::default();
    assert!(test.evaluator.write_memory(65, Immediate::Felt(Felt::ONE)).is_err());
    assert!(test.evaluator.read_memory(65, &Type::Felt).is_err());
    Ok(())
}

#[test]
fn native_addresses_cover_the_vm_cell_space() -> Result<(), Report> {
    let mut test = EvalTest::default();
    let addr = MemoryAddress::new(u32::MAX, AddressSpace::Element);
    let value = Immediate::Felt(Felt::new_unchecked((1u64 << 48) + 9));
    test.evaluator.write_memory(addr, value)?;
    assert_eq!(test.evaluator.read_memory(addr, &Type::Felt)?, Value::Immediate(value));
    assert!(test.evaluator.read_memory_elements(addr, 8).is_err());
    Ok(())
}

#[test]
fn element_snapshots_preserve_partial_tail_bytes() -> Result<(), Report> {
    let mut test = EvalTest::default();
    test.evaluator.write_memory_bytes(64, &[1, 2, 3, 4, 5, 6, 7, 8])?;
    test.evaluator.write_memory_bytes(80, &[9; 8])?;
    let elements = test.evaluator.read_memory_elements(64.into(), 6)?;
    test.evaluator.write_memory_elements(80.into(), 6, &elements)?;
    assert_eq!(test.evaluator.read_memory_bytes(80, 8)?, [1, 2, 3, 4, 5, 6, 9, 9]);
    Ok(())
}

#[test]
fn invalid_snapshot_tail_leaves_the_destination_unchanged() -> Result<(), Report> {
    let mut test = EvalTest::default();
    test.evaluator.write_memory_bytes(80, &[9; 8])?;
    let elements = [Felt::from(0x0403_0201u32), Felt::new_unchecked(1u64 << 32)];
    assert!(test.evaluator.write_memory_elements(80.into(), 6, &elements).is_err());
    assert_eq!(test.evaluator.read_memory_bytes(80, 8)?, [9; 8]);
    Ok(())
}

#[test]
fn invalid_byte_write_leaves_the_destination_unchanged() -> Result<(), Report> {
    let mut test = EvalTest::default();
    test.evaluator.write_memory(64, Immediate::U32(0x0403_0201))?;
    test.evaluator
        .write_memory(68, Immediate::Felt(Felt::new_unchecked(1u64 << 32)))?;
    assert!(test.evaluator.write_memory_bytes(64, &[9; 6]).is_err());
    assert_eq!(test.evaluator.read_memory_bytes(64, 4)?, [1, 2, 3, 4]);
    Ok(())
}

#[test]
fn last_multiword_local_uses_all_of_its_cells() -> Result<(), Report> {
    let mut test = EvalTest::named("multiword_local_cells");
    test.with_function(&[], &[Type::U128]);
    let local = test.function().borrow_mut().alloc_local(Type::U128);
    {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        let value = builder.imm(Immediate::U128(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210), span);
        builder.store_local(local, value, span)?;
        let loaded = builder.load_local(local, span)?;
        builder.ret(Some(loaded), span)?;
    }
    let callable = test.function().borrow();
    assert_eq!(
        test.evaluator.eval_callable(&*callable, [])?.as_slice(),
        &[Value::Immediate(Immediate::U128(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210))]
    );
    Ok(())
}

/// Initialize cells independently of the copy pointee type, then evaluate the HIR copy.
fn copy_cells(
    op: CopyOp,
    pointee: Type,
    space: AddressSpace,
    dst_byte_offset: u32,
    count: u32,
) -> Result<[Felt; 12], Report> {
    let initial =
        core::array::from_fn::<_, 12, _>(|index| Felt::new_unchecked((1u64 << 40) + index as u64));
    let mut test = EvalTest::named("copy_cells");
    test.with_function(&[], &[]);
    {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        for (index, value) in initial.iter().enumerate() {
            let addr = builder.u32(64 + index as u32 * 4, span);
            let ptr = builder.inttoptr(addr, Type::from(PointerType::new(Type::Felt)), span)?;
            let value = builder.felt(*value, span);
            builder.store(ptr, value, span)?;
        }
        let units = match space {
            AddressSpace::Byte => 1,
            AddressSpace::Element => 4,
        };
        let ptr_ty = Type::from(PointerType::new_with_address_space(pointee, space));
        let src_addr = builder.u32(64 / units, span);
        let dst_addr = builder.u32((64 + dst_byte_offset) / units, span);
        let src = builder.inttoptr(src_addr, ptr_ty.clone(), span)?;
        let dst = builder.inttoptr(dst_addr, ptr_ty, span)?;
        let count = builder.u32(count, span);
        match op {
            CopyOp::MemMove => {
                builder.memmove(src, dst, count, span)?;
            }
            CopyOp::MemCpy => {
                builder.memcpy(src, dst, count, span)?;
            }
        }
        builder.ret(None, span)?;
    }
    let callable = test.function().borrow();
    test.evaluator.eval_callable(&*callable, [])?;
    let mut result = [Felt::ZERO; 12];
    for (index, element) in result.iter_mut().enumerate() {
        let Value::Immediate(Immediate::Felt(value)) =
            test.evaluator.read_memory(64 + index as u32 * 4, &Type::Felt)?
        else {
            unreachable!()
        };
        *element = value;
    }
    Ok(result)
}

#[test]
fn aligned_u64_copy_preserves_full_width_cells() -> Result<(), Report> {
    let result = copy_cells(CopyOp::MemMove, Type::U64, AddressSpace::Byte, 4, 2)?;
    let expected = [0u64, 0, 1, 2, 3, 5, 6, 7, 8, 9, 10, 11]
        .map(|low| Felt::new_unchecked((1u64 << 40) + low));
    assert_eq!(result, expected);
    Ok(())
}

#[test]
fn element_address_copies_preserve_full_width_cells() -> Result<(), Report> {
    for (pointee, count) in [(Type::U8, 12), (Type::Felt, 3), (Type::U32, 3)] {
        let result = copy_cells(CopyOp::MemMove, pointee, AddressSpace::Element, 4, count)?;
        let expected = [0u64, 0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 11]
            .map(|low| Felt::new_unchecked((1u64 << 40) + low));
        assert_eq!(result, expected);
    }
    Ok(())
}

#[test]
fn byte_copy_path_rejects_full_width_cells() {
    assert!(copy_cells(CopyOp::MemMove, Type::U8, AddressSpace::Byte, 1, 12).is_err());
    assert!(copy_cells(CopyOp::MemMove, Type::U8, AddressSpace::Element, 4, 3).is_err());
    assert!(copy_cells(CopyOp::MemMove, Type::U16, AddressSpace::Byte, 4, 2).is_err());
}

#[test]
fn element_address_memcpy_rejects_overlap() {
    assert!(copy_cells(CopyOp::MemCpy, Type::Felt, AddressSpace::Element, 4, 3).is_err());
}

#[test]
fn native_memset_packs_sub_element_values() -> Result<(), Report> {
    let mut test = EvalTest::named("native_memset_packs_sub_elements");
    test.with_function(&[], &[]);
    {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        let addr = builder.u32(16, span);
        let ptr = builder.inttoptr(
            addr,
            Type::from(PointerType::new_with_address_space(Type::U16, AddressSpace::Element)),
            span,
        )?;
        let value = builder.imm(Immediate::U16(0xabcd), span);
        let count = builder.u32(3, span);
        builder.memset(ptr, count, value, span)?;
        builder.ret(None, span)?;
    }
    let callable = test.function().borrow();
    test.evaluator.eval_callable(&*callable, [])?;
    assert_eq!(
        test.evaluator.read_memory_bytes(64, 8)?,
        [0xcd, 0xab, 0xcd, 0xab, 0xcd, 0xab, 0, 0]
    );
    Ok(())
}

#[test]
fn element_pointer_sign_extending_load_uses_native_units() -> Result<(), Report> {
    let mut test = EvalTest::named("native_sign_extending_load");
    test.with_function(&[], &[Type::I32]);
    {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        let byte_addr = builder.u32(64, span);
        let byte_ptr = builder.inttoptr(byte_addr, Type::from(PointerType::new(Type::I8)), span)?;
        let value = builder.imm(Immediate::I8(-2), span);
        builder.store(byte_ptr, value, span)?;
        let native_addr = builder.u32(16, span);
        let native_ptr = builder.inttoptr(
            native_addr,
            Type::from(PointerType::new_with_address_space(Type::I8, AddressSpace::Element)),
            span,
        )?;
        let op =
            builder.builder_mut().create::<midenc_dialect_wasm::I32Load8S, _>(span)(native_ptr)?;
        let loaded = op.borrow().result().as_value_ref();
        builder.ret(Some(loaded), span)?;
    }
    let callable = test.function().borrow();
    assert_eq!(
        test.evaluator.eval_callable(&*callable, [])?.as_slice(),
        &[Value::Immediate(Immediate::I32(-2))]
    );
    Ok(())
}
