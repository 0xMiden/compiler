use super::*;

mod aggregate_pointer;

#[test]
fn mem_grow_and_size_have_u32_publication() {
    let context = Rc::new(Context::default());
    let mut stack = crate::OperandStack::new(context);
    stack.push(midenc_hir::Type::U32);
    let mut invoked = Default::default();
    let mut body = vec![];
    crate::emit::OpEmitter::new(&mut invoked, &mut body, &mut stack).mem_grow(Default::default());
    assert_eq!(stack[0].ty(), midenc_hir::Type::U32);
}

/// Independent fixtures enumerate all seventeen memory registrations.
pub(super) fn fixtures() -> Vec<(Parsed, bool)> {
    let mut out = vec![];
    for op in ["load", "store", "mem_set", "mem_cpy", "mem_move", "mem_grow", "mem_size"] {
        let (args, result, body) = match op {
            "load" => (
                "%p: ptr<u32, byte>",
                " -> u32",
                "%r = hir.load %p; builtin.ret %r : (u32);".to_string(),
            ),
            "store" => (
                "%p: ptr<u32, byte>, %v: u32",
                "",
                "hir.store %p, %v : (ptr<u32, byte>, u32); builtin.ret;".to_string(),
            ),
            "mem_set" => (
                "%p: ptr<u32, byte>, %n: u32, %v: u32",
                "",
                "hir.mem_set %p, %n, %v : (ptr<u32, byte>, u32, u32); builtin.ret;".to_string(),
            ),
            "mem_cpy" | "mem_move" => (
                "%p: ptr<u16, byte>, %q: ptr<u16, byte>, %n: u32",
                "",
                alloc::format!(
                    "hir.{op} %p, %q, %n : (ptr<u16, byte>, ptr<u16, byte>, u32); builtin.ret;"
                ),
            ),
            "mem_grow" => (
                "%n: u32",
                " -> u32",
                "%r = hir.mem_grow %n; builtin.ret %r : (u32);".to_string(),
            ),
            _ => ("", " -> u32", "%r = hir.mem_size; builtin.ret %r : (u32);".to_string()),
        };
        out.push((
            parse(&alloc::format!(
                "builtin.function public extern(\"C\") @test({args}){result} {{ {body} }};"
            )),
            false,
        ));
    }
    for (op, src, dst) in [
        ("i32_load_8s", "i8", "i32"),
        ("i32_load_16s", "i16", "i32"),
        ("i64_load_8s", "i8", "i64"),
        ("i64_load_16s", "i16", "i64"),
        ("i64_load_32s", "i32", "i64"),
    ] {
        out.push((
            parse(&alloc::format!(
                "builtin.function public extern(\"C\") @test(%p: ptr<{src}, byte>) -> {dst} {{ %r \
                 = wasm.{op} %p; builtin.ret %r : ({dst}); }};"
            )),
            false,
        ));
    }
    for (op, args, result, body) in [
        (
            "load_local",
            "",
            " -> u32",
            "%r = hir.load_local <{ local = #builtin.local_variable<0, u32> }>; builtin.ret %r : \
             (u32);",
        ),
        (
            "store_local",
            "%v: u32",
            "",
            "hir.store_local %v <{ local = #builtin.local_variable<0, u32> }> : (u32); \
             builtin.ret;",
        ),
        (
            "local_address",
            "",
            " -> ptr<u32, element>",
            "%r = hir.local_address <{ local = #builtin.local_variable<0, u32> }>; builtin.ret %r \
             : (ptr<u32, element>);",
        ),
    ] {
        let mut f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @{op}({args}){result} {{ {body} }};"
        ));
        f.function.borrow_mut().alloc_local(midenc_hir::Type::U32);
        out.push((f, false));
    }
    for op in ["alloca", "local_byte_address"] {
        let mut f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @{op}() -> ptr<u32, byte> {{ %p = hir.{op} <{{ \
             local = #builtin.local_variable<0, u32> }}>; builtin.ret %p : (ptr<u32, byte>); }};"
        ));
        f.function.borrow_mut().alloc_local(midenc_hir::Type::U32);
        out.push((f, false));
    }
    out
}

#[test]
fn all_registered_memory_operations_select_exact_source() {
    for (f, _) in fixtures() {
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        verify::verify(&plan, &frame, body).unwrap();
    }
}

fn memory_function(ty: &str, address_space: &str, body: &str, result: bool) -> Parsed {
    let pointer = alloc::format!("ptr<{ty}{address_space}>");
    let result_type = if result {
        alloc::format!(" -> {ty}")
    } else {
        "".into()
    };
    parse(&alloc::format!(
        "builtin.function public extern(\"C\") @memory(%p: {pointer}, %v: {ty}){result_type} {{ \
         {body} }};"
    ))
}

#[test]
fn typed_memory_matrix_matches_vm() {
    for (ty, value) in [
        ("i1", vec![1]),
        ("u8", vec![0xad]),
        ("i8", vec![0xfe]),
        ("u16", vec![0xbeef]),
        ("i16", vec![0xffee]),
        ("u32", vec![0xaabbccdd]),
        ("i32", vec![0xffeeddcc]),
        ("u64", vec![0x12345678, 0xabcdef01]),
        ("i64", vec![0x10203040, 0xfedcba98]),
        ("u128", vec![1, 2, 3, 0xffffffff]),
        ("i128", vec![9, 8, 7, 0xfffffffe]),
        ("felt", vec![0xffff_ffff_0000_0000]),
        ("ptr<u32, byte>", vec![0x1234]),
    ] {
        for space in [", byte", ", element"] {
            let body = alloc::format!(
                "hir.store %p, %v : (ptr<{ty}{space}>, {ty}); %r = hir.load %p; builtin.ret %r : \
                 ({ty});"
            );
            let f = memory_function(ty, space, &body, true);
            for offset in 0..if space == ", byte" && ty != "felt" {
                4
            } else {
                1
            } {
                let mut inputs = vec![if space == ", byte" { 256 + offset } else { 64 }];
                inputs.extend(&value);
                execute_linked_memory(
                    &f,
                    &inputs,
                    &value,
                    "push.0 mem_store.64 push.0 mem_store.65 push.0 mem_store.66 push.0 \
                     mem_store.67 push.0 mem_store.68",
                    "",
                )
                .unwrap();
            }
        }
    }
    let f = memory_function(
        "u8",
        ", byte",
        "hir.store %p, %v : (ptr<u8, byte>, u8); %r = hir.load %p; builtin.ret %r : (u8);",
        true,
    );
    execute_linked_memory(
        &f,
        &[257, 0xab],
        &[0xab],
        "push.287454020 mem_store.64",
        "mem_load.64 push.287484740 assert_eq",
    )
    .unwrap();
}

#[test]
fn byte_pointer_stores_merge_into_full_field_cells() {
    // A cell's low bytes are 44 33 22 11. Its high bits may come from an earlier Felt store,
    // including reuse of a procedure frame. Only bytes covered by this store may change.
    let initial = (1usize << 40) | 0x1122_3344;
    let mut failures = vec![];
    for (ty, value, bytes) in [
        ("u8", vec![0xaa], vec![0xaa]),
        ("i8", vec![0xfe], vec![0xfe]),
        ("u16", vec![0xbbaa], vec![0xaa, 0xbb]),
        ("u32", vec![0xddcc_bbaa], vec![0xaa, 0xbb, 0xcc, 0xdd]),
        (
            "u64",
            vec![0xddcc_bbaa, 0x8877_6655],
            vec![0xaa, 0xbb, 0xcc, 0xdd, 0x55, 0x66, 0x77, 0x88],
        ),
        (
            "u128",
            vec![0xddcc_bbaa, 0x8877_6655, 0x0403_0201, 0x0807_0605],
            vec![0xaa, 0xbb, 0xcc, 0xdd, 0x55, 0x66, 0x77, 0x88, 1, 2, 3, 4, 5, 6, 7, 8],
        ),
    ] {
        for space in [", byte", ", element"] {
            let f = memory_function(
                ty,
                space,
                &alloc::format!("hir.store %p, %v : (ptr<{ty}{space}>, {ty}); builtin.ret;"),
                false,
            );
            for offset in 0..if space == ", byte" { 4 } else { 1 } {
                let mut expected_bytes = [0x44, 0x33, 0x22, 0x11].repeat(6);
                expected_bytes[offset..offset + bytes.len()].copy_from_slice(&bytes);
                let initialize = (0..6)
                    .map(|i| alloc::format!("push.{initial} mem_store.{}", 64 + i))
                    .collect::<Vec<_>>()
                    .join(" ");
                let observe = expected_bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, chunk)| {
                        let touched = i * 4 < offset + bytes.len() && (i + 1) * 4 > offset;
                        let expected = if touched {
                            u32::from_le_bytes(*chunk) as usize
                        } else {
                            initial
                        };
                        alloc::format!("mem_load.{} push.{expected} assert_eq", 64 + i)
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut inputs = vec![if space == ", byte" { 256 + offset } else { 64 }];
                inputs.extend(&value);
                if let Err(error) = execute_linked_memory(&f, &inputs, &[], &initialize, &observe) {
                    failures.push(alloc::format!("{ty}{space} offset {offset}: {error}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn byte_pointer_loads_extract_low_bytes_from_full_field_cells() {
    let mut failures = vec![];
    let initial = (1usize << 40) | 0x4444_4444;
    let initialize = (0..5)
        .map(|i| alloc::format!("push.{initial} mem_store.{}", 64 + i))
        .collect::<Vec<_>>()
        .join(" ");
    let observe = (0..5)
        .map(|i| alloc::format!("mem_load.{} push.{initial} assert_eq", 64 + i))
        .collect::<Vec<_>>()
        .join(" ");
    for (ty, value, lanes) in [
        ("i1", 0, 1),
        ("u8", 0x44, 1),
        ("i8", 0x44, 1),
        ("u16", 0x4444, 1),
        ("i16", 0x4444, 1),
        ("u32", 0x4444_4444, 1),
        ("u64", 0x4444_4444, 2),
        ("u128", 0x4444_4444, 4),
    ] {
        for space in ["byte", "element"] {
            let f = parse(&alloc::format!(
                "builtin.function public extern(\"C\") @load(%p: ptr<{ty}, {space}>) -> {ty} {{ \
                 %v = hir.load %p; builtin.ret %v : ({ty}); }};"
            ));
            for offset in 0..if space == "byte" { 4 } else { 1 } {
                let address = if space == "byte" { 256 + offset } else { 64 };
                // Whole-cell aligned loads retain full field values.
                let expected = vec![
                    if offset == 0 && value == 0x4444_4444 {
                        initial
                    } else {
                        value
                    };
                    lanes
                ];
                if let Err(error) =
                    execute_linked_memory(&f, &[address], &expected, &initialize, &observe)
                {
                    failures.push(alloc::format!("{ty} {space} offset {offset}: {error}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn signed_loads_match_vm() {
    for (op, src, dst, bits) in [
        ("i32_load_8s", "i8", "i32", 8),
        ("i32_load_16s", "i16", "i32", 16),
        ("i64_load_8s", "i8", "i64", 8),
        ("i64_load_16s", "i16", "i64", 16),
        ("i64_load_32s", "i32", "i64", 32),
    ] {
        let f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @signed(%p: ptr<{src}, byte>) -> {dst} {{ %r = \
             wasm.{op} %p; builtin.ret %r : ({dst}); }};"
        ));
        for raw in [0u64, 1, 1 << (bits - 1), (1u64 << bits) - 1] {
            for offset in 0..4 {
                let signed = ((raw << (64 - bits)) as i64) >> (64 - bits);
                let expected = if dst == "i32" {
                    vec![signed as u32 as usize]
                } else {
                    vec![signed as u32 as usize, (signed as u64 >> 32) as usize]
                };
                let wide = (raw as u128) << (offset * 8);
                let initialize = alloc::format!(
                    "push.{} mem_store.64 push.{} mem_store.65",
                    wide as u32,
                    (wide >> 32) as u32
                );
                execute_linked_memory(&f, &[256 + offset], &expected, &initialize, "").unwrap();
            }
        }
    }
}

#[test]
fn exact_memory_types_arity_and_local_authority_are_checked() {
    use midenc_hir::{Type, Value};
    let f = parse(
        "builtin.function public extern(\"C\") @load(%p: ptr<u32, byte>) { %r = hir.load %p; \
         builtin.ret; };",
    );
    let mut result =
        f.borrow().entry_block().borrow().body().iter().next().unwrap().results().all()[0];
    result.borrow_mut().set_type(Type::I32);
    assert_malformed_source_rejected(&f, &SelectEnv::default());
    for (mut f, _) in fixtures() {
        if f.borrow().name().as_str() == "load_local"
            || f.borrow().name().as_str() == "local_address"
            || f.borrow().name().as_str() == "store_local"
        {
            f.function.borrow_mut().get_locals_mut().remove(0);
            assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
        }
    }
}

#[test]
fn selected_fill_reuses_approved_native_algorithm() {
    for (ty, value) in [
        ("u8", vec![0xa7]),
        ("u16", vec![0xabcd]),
        ("u32", vec![0xaabbccdd]),
        ("u64", vec![0x12345678, 0xabcdef01]),
        ("u128", vec![1, 2, 3, 4]),
        ("felt", vec![0xffff_ffff_0000_0000]),
    ] {
        for space in [", byte", ", element"] {
            let f = parse(&alloc::format!(
                "builtin.function public extern(\"C\") @fill(%p: ptr<{ty}{space}>, %n: u32, %v: \
                 {ty}) {{ hir.mem_set %p, %n, %v : (ptr<{ty}{space}>,u32,{ty}); builtin.ret; }};"
            ));
            let mut inputs = vec![if space == ", byte" { 256 } else { 64 }, 3];
            inputs.extend(&value);
            let bytes = match ty {
                "u8" => 1,
                "u16" => 2,
                "u64" => 8,
                "u128" => 16,
                _ => 4,
            };
            let mut cells = [0u64; 13];
            if ty == "felt" {
                cells[..3].fill(value[0] as u64);
            } else {
                for n in 0..3 {
                    for (lane, v) in value.iter().enumerate() {
                        for b in 0..bytes.min(4) {
                            let pos = n * bytes + lane * 4 + b;
                            cells[pos / 4] |= (((*v as u64) >> (8 * b)) & 255) << (8 * (pos % 4));
                        }
                    }
                }
            }
            let init = (0..13)
                .map(|i| alloc::format!("push.0 mem_store.{}", 64 + i))
                .collect::<Vec<_>>()
                .join(" ");
            let observe = cells
                .iter()
                .enumerate()
                .map(|(i, v)| alloc::format!("mem_load.{} push.{v} assert_eq", 64 + i))
                .collect::<Vec<_>>()
                .join(" ");
            execute_linked_memory(&f, &inputs, &[], &init, &observe).unwrap();
        }
    }
}

#[test]
fn selected_copy_paths_match_existing_semantics() {
    for ty in ["u8", "u16", "u32", "u64", "u128", "array<u32; 8>"] {
        for op in ["mem_cpy", "mem_move"] {
            let source_ty = if ty == "array<u32; 8>" { "u32" } else { ty };
            let mut f = parse(&alloc::format!(
                "builtin.function public extern(\"C\") @copy(%p: ptr<{source_ty}, byte>, %q: \
                 ptr<{source_ty}, byte>, %n: u32) {{ hir.{op} %p, %q, %n : (ptr<{source_ty}, \
                 byte>, ptr<{source_ty}, byte>, u32); builtin.ret; }};"
            ));
            if ty == "array<u32; 8>" {
                let ptr = midenc_hir::Type::from(midenc_hir::PointerType::new(
                    midenc_hir::Type::from(midenc_hir::ArrayType::new(midenc_hir::Type::U32, 8)),
                ));
                retype_parameters(&mut f, &[ptr.clone(), ptr, midenc_hir::Type::U32]);
            }
            let size = match ty {
                "u8" => 1,
                "u16" => 2,
                "u32" => 4,
                "u64" => 8,
                "u128" => 16,
                _ => 32,
            };
            let init = (0..32)
                .map(|i| alloc::format!("push.{} mem_store.{}", 0x10203040u64 + i, 64 + i))
                .collect::<Vec<_>>()
                .join(" ");
            for count in [0, 1, 2] {
                let bytes = count * size;
                let host =
                    (0..32).flat_map(|i| (0x10203040u32 + i).to_le_bytes()).collect::<Vec<_>>();
                let mut expected = host.clone();
                expected.copy_within(0..bytes, 64);
                let observe = expected
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        alloc::format!(
                            "mem_load.{} push.{} assert_eq",
                            64 + i,
                            u32::from_le_bytes(*c)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                execute_linked_memory(&f, &[256, 320, count], &[], &init, &observe).unwrap();
            }
        }
    }
}

#[test]
fn heap_memory_executes_u32_results_and_traps() {
    let grow = parse(
        "builtin.function public extern(\"C\") @grow(%n: u32) -> u32 { %r = hir.mem_grow %n; \
         builtin.ret %r : (u32); };",
    );
    let size = parse(
        "builtin.function public extern(\"C\") @size() -> u32 { %r = hir.mem_size; builtin.ret %r \
         : (u32); };",
    );
    let init = "push.65536 exec.::intrinsics::mem::heap_init";
    for (pages, expected, new_size) in
        [(0, 0, 0), (2, 0, 2), (u32::MAX as usize, u32::MAX as usize, 0)]
    {
        let observe = alloc::format!(
            "mem_load.1073741825 push.{new_size} assert_eq mem_load.1073741824 push.{} assert_eq",
            65536 + new_size * 65536
        );
        execute_linked_memory(&grow, &[pages], &[expected], init, &observe).unwrap();
    }
    execute_linked_memory(&size, &[], &[0], init, "").unwrap();
    assert!(execute_linked_memory(&size, &[], &[], "", "").is_err());
    assert!(execute_linked_memory(&grow, &[0], &[], "", "").is_err());
}

#[test]
fn local_memory_uses_final_frame_and_preserves_spills() {
    use midenc_hir::Type;
    let mut f = parse(
        "builtin.function public extern(\"C\") @local(%v: u64) -> u64 { hir.store_local %v <{ \
         local = #builtin.local_variable<1, u64> }> : (u64); %p = hir.local_address <{ local = \
         #builtin.local_variable<1, u64> }>; %r = hir.load %p; builtin.ret %r : (u64); };",
    );
    f.function.borrow_mut().alloc_local(Type::U128);
    f.function.borrow_mut().alloc_local(Type::U64);
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    assert_eq!(frame.resolve(FrameRef::SourceLocal(1)).unwrap(), 4);
    verify::verify(&plan, &frame, body).unwrap();
    execute_linked(&f, &[0x12345678, 0xabcdef01], &[0x12345678, 0xabcdef01]);
}

#[test]
fn mutated_memory_calls_control_and_publication_reject() {
    let f = parse(
        "builtin.function public extern(\"C\") @load(%p: ptr<u64, byte>) -> u64 { %r = hir.load \
         %p; builtin.ret %r : (u64); };",
    );
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    for mutation in 0..5 {
        let mut bad = body.clone();
        let call = bad
            .ops_mut()
            .iter()
            .position(|op| {
                matches!(
                    op,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::Exec(_),
                        ..
                    }
                )
            })
            .unwrap();
        match mutation {
            0 => {
                let AnnotatedOp::Instruction { instruction, .. } = &mut bad.ops_mut()[call] else {
                    unreachable!()
                };
                *instruction = crate::masm::Instruction::Exec(crate::lower::raw::target(
                    "::intrinsics::mem::load_sw",
                ));
            }
            1 => {
                bad.ops_mut().remove(call);
            }
            2 => {
                let copy = bad.ops()[call].clone();
                bad.ops_mut().insert(call, copy);
            }
            3 => {
                let end = bad
                    .ops_mut()
                    .iter()
                    .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitEnd(_))))
                    .unwrap();
                bad.ops_mut().remove(end);
            }
            _ => {
                bad.ops_mut().insert(
                    call,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::Swap2,
                        unit: SelectedStepId(0),
                    },
                );
            }
        }
        assert!(verify::verify(&plan, &frame, bad).is_err(), "mutation {mutation}");
    }
    for (f, _) in fixtures() {
        let function = f.borrow();
        let mut selected = select_function(&function, &SelectEnv::default()).unwrap();
        let unit = selected.segments.iter_mut().flat_map(|s| &mut s.units).next().unwrap();
        unit.effects.may_trap = false;
        // LocalAddress has no trap; its effect mutation is handled separately by coverage.
        if !matches!(function.name().as_str(), "local_address" | "alloca" | "local_byte_address") {
            assert!(
                check_selection(
                    selected,
                    &SourceContext::Hir {
                        function: &function,
                        env: &SelectEnv::default()
                    }
                )
                .is_err()
            );
        }
    }
}

/// Retype real source parameters and their immutable signature together; no evaluator stand-in.
fn retype_parameters(f: &mut Parsed, types: &[midenc_hir::Type]) {
    use midenc_hir::Value;
    for (i, ty) in types.iter().enumerate() {
        f.function.borrow_mut().get_signature_mut().param_mut(i).unwrap().ty = ty.clone();
        let mut argument = f.borrow().entry_block().borrow().arguments()[i];
        argument.borrow_mut().set_type(ty.clone());
    }
}

#[test]
fn packed_store_fill_and_local_forms_preserve_approved_domain() {
    use midenc_hir::{
        AddressSpace, ArrayType, Builder, BuilderExt, CallConv, EnumType, FunctionType, OpBuilder,
        PointerType, SourceSpan, StructType, Type, TypeRepr, Variant,
    };
    let types = [
        Type::from(ArrayType::new(Type::U32, 1)),
        Type::from(StructType::new_with_repr(TypeRepr::Transparent, [Type::U32])),
        Type::from(
            EnumType::new("PackedTag".into(), Type::U32, [Variant::c_like("One".into(), Some(1))])
                .unwrap(),
        ),
        Type::from(FunctionType::new(CallConv::Fast, [], [])),
        Type::from(ArrayType::new(Type::U8, 3)),
        // Boolean elements are byte-aligned, giving these arrays 9-, 17-, and
        // 25-bit footprints rather than a whole number of bytes.
        Type::from(ArrayType::new(Type::I1, 2)),
        Type::from(ArrayType::new(Type::I1, 3)),
        Type::from(ArrayType::new(Type::I1, 4)),
        Type::from(ArrayType::new(Type::Felt, 1)),
    ];
    for ty in types {
        for element in [false, true] {
            let ptr = Type::from(PointerType::new_with_address_space(
                ty.clone(),
                if element {
                    AddressSpace::Element
                } else {
                    AddressSpace::Byte
                },
            ));
            for fill in [false, true] {
                let mut f = if fill {
                    parse(
                        "builtin.function public extern(\"C\") @packed(%p: ptr<u32, byte>, %n: \
                         u32, %v: u32) { hir.mem_set %p, %n, %v : (ptr<u32, byte>, u32, u32); \
                         builtin.ret; };",
                    )
                } else {
                    parse(
                        "builtin.function public extern(\"C\") @packed(%p: ptr<u32, byte>, %v: \
                         u32) { hir.store %p, %v : (ptr<u32, byte>, u32); builtin.ret; };",
                    )
                };
                let types = if fill {
                    vec![ptr.clone(), Type::U32, ty.clone()]
                } else {
                    vec![ptr.clone(), ty.clone()]
                };
                retype_parameters(&mut f, &types);
                let bits = ty.size_in_bits();
                let value = 0x12345678usize
                    & if bits < 32 {
                        (1usize << bits) - 1
                    } else {
                        u32::MAX as usize
                    };
                let count = if fill { 3 } else { 1 };
                for offset in 0..if element { 1 } else { 4 } {
                    let base = if element { 64 } else { 256 + offset };
                    let inputs = if fill {
                        vec![base, count, value]
                    } else {
                        vec![base, value]
                    };
                    if !fill
                        && offset != 0
                        && matches!(&ty, Type::Array(array) if array.element_type() == &Type::Felt)
                    {
                        // Typed aggregate stores preserve whole Felts and reject splitting them.
                        // Fill eligibility and behavior are a separate contract.
                        let error = execute_linked_memory(&f, &inputs, &[], "", "").unwrap_err();
                        let code = miden_core::mast::error_code_from_msg(
                            "aggregate Felt access requires an element-aligned pointer",
                        );
                        assert!(error.to_string().contains(&code.to_string()), "{error}");
                        continue;
                    }
                    let mut bytes = [0xa5u8; 16];
                    for n in 0..count {
                        let bit = offset * 8 + n * ty.size_in_bytes() * 8;
                        for b in 0..bits {
                            if value & (1usize << b) != 0 {
                                bytes[(bit + b) / 8] |= 1 << ((bit + b) % 8);
                            } else {
                                bytes[(bit + b) / 8] &= !(1 << ((bit + b) % 8));
                            }
                        }
                    }
                    let observe = bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            alloc::format!(
                                "mem_load.{} push.{} assert_eq",
                                64 + i,
                                u32::from_le_bytes(*c)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    execute_linked_memory(
                        &f,
                        &inputs,
                        &[],
                        "push.2779096485 mem_store.64 push.2779096485 mem_store.65 \
                         push.2779096485 mem_store.66 push.2779096485 mem_store.67",
                        &observe,
                    )
                    .unwrap_or_else(|e| {
                        panic!(
                            "{ty}, fill={fill}, element={element}, offset={offset}, bits={bits}, \
                             stride={}: {e:?}",
                            ty.size_in_bytes()
                        )
                    });
                }
            }
        }
        let mut f =
            parse("builtin.function public extern(\"C\") @local_packed(%v: u32) { builtin.ret; };");
        retype_parameters(&mut f, core::slice::from_ref(&ty));
        let local = f.function.borrow_mut().alloc_local(ty.clone());
        let value = f.borrow().entry_block().borrow().arguments()[0];
        let terminator = f.borrow().entry_block().borrow().terminator().unwrap();
        let mut builder = OpBuilder::new(f.context.clone());
        builder.set_insertion_point_before(terminator);
        builder.create::<crate::hir::StoreLocal, _>(SourceSpan::UNKNOWN)(local, value).unwrap();
        execute_linked(&f, &[1], &[]);
        // Pointer loads accept the same bounded aggregate representation as typed locals.
        let mut load = parse(
            "builtin.function public extern(\"C\") @packed_load(%p: ptr<u32, byte>) { %r = \
             hir.load %p; builtin.ret; };",
        );
        retype_parameters(&mut load, &[Type::from(PointerType::new(ty.clone()))]);
        use midenc_hir::Value;
        let mut result = load
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .iter()
            .next()
            .unwrap()
            .results()
            .all()[0];
        result.borrow_mut().set_type(ty);
        select_function(&load.borrow(), &SelectEnv::default()).unwrap();
    }
}

#[test]
fn wrong_valid_source_local_instruction_is_caught_by_vm_semantics() {
    use midenc_hir::Type;
    let mut f = parse(
        "builtin.function public extern(\"C\") @locals(%a: u32, %b: u32) -> u32 { hir.store_local \
         %a <{ local = #builtin.local_variable<0, u32> }> : (u32); hir.store_local %b <{ local = \
         #builtin.local_variable<1, u32> }> : (u32); %r = hir.load_local <{ local = \
         #builtin.local_variable<1, u32> }>; builtin.ret %r : (u32); };",
    );
    f.function.borrow_mut().alloc_local(Type::U32);
    f.function.borrow_mut().alloc_local(Type::U32);
    execute_linked(&f, &[17, 29], &[29]);
    let result = execute_linked_memory_mutate(&f, &[17, 29], &[29], "", "", |body| {
        let instruction = body
            .ops_mut()
            .iter_mut()
            .rev()
            .find_map(|op| match op {
                AnnotatedOp::Instruction {
                    instruction: i @ crate::masm::Instruction::Locaddr(_),
                    ..
                } => Some(i),
                _ => None,
            })
            .unwrap();
        *instruction = crate::masm::Instruction::Locaddr(0u16.into());
    });
    // Replay preserves the real local address token. Choosing the right valid source local is
    // algorithm semantics; the actual VM return-value assertion detects the wrong one.
    assert!(result.is_err());
}

#[test]
fn local_memory_preserves_live_spill_sentinels() {
    use midenc_hir::Type;
    let args = (0..20).map(|i| alloc::format!("%s{i}: u32")).collect::<Vec<_>>().join(", ");
    let results = (0..20).map(|_| "u32").chain(["u64"]).collect::<Vec<_>>().join(", ");
    let values = (0..20)
        .map(|i| alloc::format!("%s{i}"))
        .chain(["%r".into()])
        .collect::<Vec<_>>()
        .join(", ");
    let mut f = parse(&alloc::format!(
        "builtin.function public extern(\"C\") @pressure({args}, %v: u64) -> ({results}) {{ \
         hir.store_local %v <{{ local = #builtin.local_variable<1, u64> }}> : (u64); %r = \
         hir.load_local <{{ local = #builtin.local_variable<1, u64> }}>; builtin.ret {values} : \
         ({results}); }};"
    ));
    f.function.borrow_mut().alloc_local(Type::U128);
    f.function.borrow_mut().alloc_local(Type::U64);
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    assert!(!frame.spills().is_empty());
    verify::verify(&plan, &frame, body).unwrap();
    let inputs = (0..20).map(|i| 100 + i).chain([0x12345678, 0xabcdef01]).collect::<Vec<_>>();
    execute_linked(&f, &inputs, &inputs);
}

#[test]
fn memory_source_result_arity_and_exact_types_reject_without_inference() {
    use midenc_hir::{SourceSpan, Type, Value};
    for mutation in 0..3 {
        for (f, _) in fixtures() {
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            if mutation == 0 && !op.borrow().results().is_empty() {
                let mut result = op.borrow().results().all()[0];
                result.borrow_mut().set_type(Type::I128);
            } else if mutation == 1 && !op.borrow().results().is_empty() {
                let last = op.borrow().results().groups().count() - 1;
                op.borrow_mut().results_mut().group_mut(last).pop().unwrap();
            } else {
                let n = op.borrow().results().len();
                let extra = f.context.make_result(SourceSpan::UNKNOWN, Type::U32, op, n as u16);
                op.borrow_mut().results_mut().push(extra);
            }
            assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
        }
    }
    for (body, args) in [
        ("hir.store %p, %v : (ptr<u32, byte>, i32);", "%p: ptr<u32, byte>, %v: i32"),
        (
            "hir.mem_set %p, %n, %v : (ptr<u32, byte>, u32, i32);",
            "%p: ptr<u32, byte>, %n: u32, %v: i32",
        ),
    ] {
        let f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @wrong({args}) {{ {body} builtin.ret; }};"
        ));
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
}

#[test]
fn selected_fill_range_and_terminal_native_cell_match_vm() {
    for (ty, value, count, expected) in [
        ("u8", 0xab, 4, 0xababababu32),
        ("u16", 0xabcd, 2, 0xabcdabcd),
        ("u32", 0x12345678, 1, 0x12345678),
    ] {
        let f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @end(%p: ptr<{ty}, element>, %n: u32, %v: \
             {ty}) {{ hir.mem_set %p, %n, %v : (ptr<{ty}, element>, u32, {ty}); builtin.ret; }};"
        ));
        let init = "push.0 mem_store.4294967295";
        let observe = alloc::format!("mem_load.4294967295 push.{expected} assert_eq");
        execute_linked_memory(&f, &[u32::MAX as usize, count, value], &[], init, &observe).unwrap();
        assert!(
            execute_linked_memory(&f, &[u32::MAX as usize, count + 1, value], &[], init, "")
                .is_err()
        );
        execute_linked_memory(
            &f,
            &[u32::MAX as usize, 0, value],
            &[],
            init,
            "mem_load.4294967295 assertz",
        )
        .unwrap();
    }
    let f = parse(
        "builtin.function public extern(\"C\") @end(%p: ptr<u8, byte>, %n: u32, %v: u8) { \
         hir.mem_set %p, %n, %v : (ptr<u8, byte>, u32, u8); builtin.ret; };",
    );
    execute_linked_memory(&f, &[0xfffffffc, 0, 1], &[], "", "").unwrap();
    assert!(execute_linked_memory(&f, &[0xfffffffd, 0, 1], &[], "", "").is_err());
    assert!(execute_linked_memory(&f, &[0xfffffffc, 1, 1], &[], "", "").is_err());
}

#[test]
fn selected_copy_overlap_direction_range_and_alignment_match_vm() {
    for ty in ["u8", "u16", "u32", "u64"] {
        let size = match ty {
            "u8" => 1,
            "u16" => 2,
            "u32" => 4,
            _ => 8,
        };
        for op in ["mem_cpy", "mem_move"] {
            let f = parse(&alloc::format!(
                "builtin.function public extern(\"C\") @copy(%p: ptr<{ty}, byte>, %q: ptr<{ty}, \
                 byte>, %n: u32) {{ hir.{op} %p, %q, %n : (ptr<{ty}, byte>, ptr<{ty}, byte>, \
                 u32); builtin.ret; }};"
            ));
            let init = (0..12)
                .map(|i| alloc::format!("push.{} mem_store.{}", 0x10203040u32 + i, 64 + i))
                .collect::<Vec<_>>()
                .join(" ");
            for (src, dst) in [(256, 256 + size), (256 + size, 256)] {
                if op == "mem_cpy" {
                    assert!(execute_linked_memory(&f, &[src, dst, 3], &[], &init, "").is_err());
                } else {
                    let mut bytes =
                        (0..12).flat_map(|i| (0x10203040u32 + i).to_le_bytes()).collect::<Vec<_>>();
                    bytes.copy_within(src - 256..src - 256 + 3 * size, dst - 256);
                    let observe = bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            alloc::format!(
                                "mem_load.{} push.{} assert_eq",
                                64 + i,
                                u32::from_le_bytes(*c)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    execute_linked_memory(&f, &[src, dst, 3], &[], &init, &observe).unwrap();
                }
            }
            execute_linked_memory(&f, &[u32::MAX as usize, u32::MAX as usize, 0], &[], "", "")
                .unwrap();
            assert!(execute_linked_memory(&f, &[u32::MAX as usize, 256, 1], &[], "", "").is_err());
            assert!(execute_linked_memory(&f, &[256, u32::MAX as usize, 1], &[], "", "").is_err());
            assert!(
                execute_linked_memory(&f, &[256, 320, u32::MAX as usize], &[], "", "").is_err()
            );
        }
    }
    for op in ["mem_cpy", "mem_move"] {
        let f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @word(%p: ptr<u128, byte>, %q: ptr<u128, \
             byte>, %n: u32) {{ hir.{op} %p, %q, %n : (ptr<u128, byte>, ptr<u128, byte>, u32); \
             builtin.ret; }};"
        ));
        assert!(execute_linked_memory(&f, &[257, 320, 0], &[], "", "").is_err());
    }
}

#[test]
fn parsed_and_bound_local_authority_is_checked_before_emission() {
    use midenc_hir::{Builder, BuilderExt, OpBuilder, SourceSpan, Type};
    for declaration in [None, Some(Type::I32)] {
        let mut f = parse(
            "builtin.function public extern(\"C\") @mistyped() -> u32 { %r = hir.load_local <{ \
             local = #builtin.local_variable<0, u32> }>; builtin.ret %r : (u32); };",
        );
        if let Some(ty) = declaration {
            f.function.borrow_mut().alloc_local(ty);
        }
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
    let mut owner = parse("builtin.function public extern(\"C\") @owner() { builtin.ret; };");
    let foreign = owner.function.borrow_mut().alloc_local(Type::U32);
    let mut f = parse("builtin.function public extern(\"C\") @foreign() { builtin.ret; };");
    f.function.borrow_mut().alloc_local(Type::U32);
    let terminator = f.borrow().entry_block().borrow().terminator().unwrap();
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(terminator);
    builder.create::<crate::hir::LoadLocal, _>(SourceSpan::UNKNOWN)(foreign).unwrap();
    assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
}

#[test]
fn packed_locals_do_not_authorize_aggregate_pointer_copies() {
    use midenc_hir::{ArrayType, Builder, BuilderExt, OpBuilder, PointerType, SourceSpan, Type};
    for size in [2, 3, 4] {
        let ty = Type::from(ArrayType::new(Type::U8, size));
        for op in ["mem_cpy", "mem_move"] {
            let mut f = parse(&alloc::format!(
                "builtin.function public extern(\"C\") @copy(%p: ptr<u32, byte>, %q: ptr<u32, \
                 byte>, %n: u32) {{ hir.{op} %p, %q, %n : (ptr<u32, byte>, ptr<u32, byte>, u32); \
                 builtin.ret; }};"
            ));
            let pointer = Type::from(PointerType::new(ty.clone()));
            retype_parameters(&mut f, &[pointer.clone(), pointer, Type::U32]);
            assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
        }
        let mut f = parse("builtin.function public extern(\"C\") @local() { builtin.ret; };");
        let local = f.function.borrow_mut().alloc_local(ty);
        let terminator = f.borrow().entry_block().borrow().terminator().unwrap();
        let mut builder = OpBuilder::new(f.context.clone());
        builder.set_insertion_point_before(terminator);
        builder.create::<crate::hir::LocalAddress, _>(SourceSpan::UNKNOWN)(local).unwrap();
        execute_linked(&f, &[], &[]);
        builder.create::<crate::hir::LoadLocal, _>(SourceSpan::UNKNOWN)(local).unwrap();
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_ok());
    }
}

#[test]
fn wholeword_felt_wrappers_retain_full_field_values() {
    use midenc_hir::{ArrayType, PointerType, StructType, Type, TypeRepr};
    for ty in [
        Type::from(ArrayType::new(Type::Felt, 1)),
        Type::from(StructType::new_with_repr(TypeRepr::Transparent, [Type::Felt])),
    ] {
        let mut f = parse(
            "builtin.function public extern(\"C\") @wrapper(%p: ptr<u32, byte>, %n: u32, %v: u32) \
             { hir.mem_set %p, %n, %v : (ptr<u32, byte>,u32,u32); builtin.ret; };",
        );
        retype_parameters(&mut f, &[Type::from(PointerType::new(ty.clone())), Type::U32, ty]);
        let value = 0xffff_ffff_0000_0000usize;
        execute_linked_memory(
            &f,
            &[256, 2, value],
            &[],
            "",
            &alloc::format!(
                "mem_load.64 push.{value} assert_eq mem_load.65 push.{value} assert_eq"
            ),
        )
        .unwrap();
    }
}

#[test]
fn aligned_copy_fast_paths_preserve_full_field_cells() {
    for (ty, count, cells) in [("u8", 4, 1), ("u32", 1, 1), ("u128", 1, 4)] {
        for op in ["mem_cpy", "mem_move"] {
            let f = parse(&alloc::format!(
                "builtin.function public extern(\"C\") @full(%p: ptr<{ty}, byte>, %q: ptr<{ty}, \
                 byte>, %n: u32) {{ hir.{op} %p, %q, %n : (ptr<{ty}, byte>,ptr<{ty}, byte>,u32); \
                 builtin.ret; }};"
            ));
            let initialize = (0..cells)
                .map(|i| {
                    alloc::format!("push.{} mem_store.{}", 0xffff_ffff_0000_0000u64 - i, 64 + i)
                })
                .collect::<Vec<_>>()
                .join(" ");
            let observe = (0..cells)
                .map(|i| {
                    alloc::format!(
                        "mem_load.{} push.{} assert_eq",
                        80 + i,
                        0xffff_ffff_0000_0000u64 - i
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            execute_linked_memory(&f, &[256, 320, count], &[], &initialize, &observe).unwrap();
        }
    }
}

#[test]
fn selected_u16_uses_only_its_actual_memory_footprint() {
    let f = parse(
        "builtin.function public extern(\"C\") @u16(%p: ptr<u16, byte>) -> u16 { %r = hir.load \
         %p; builtin.ret %r : (u16); };",
    );
    let neighbor = (1u64 << 40) + 7;
    let init = alloc::format!("push.287454020 mem_store.64 push.{neighbor} mem_store.65");
    for (offset, expected) in [(0, 0x3344), (1, 0x2233), (2, 0x1122), (3, 0x0711)] {
        execute_linked_memory(
            &f,
            &[256 + offset],
            &[expected],
            &init,
            &alloc::format!("mem_load.65 push.{neighbor} assert_eq"),
        )
        .unwrap();
    }
}

#[test]
fn aligned_full_cell_store_has_only_its_required_memory_capability() {
    let f = parse(
        "builtin.function public extern(\"C\") @store(%p: ptr<u32, element>, %v: u32) { hir.store \
         %p, %v : (ptr<u32, element>,u32); builtin.ret; };",
    );
    let selected = select_function(&f.borrow(), &SelectEnv::default()).unwrap();
    let unit = selected.segments.iter().flat_map(|s| &s.units).next().unwrap();
    assert_eq!(unit.effects.memory, SourceMemoryEffects::WRITE);
}

#[test]
fn memory_missing_and_extra_operands_are_checked_before_emission() {
    use midenc_hir::{Builder, BuilderExt, Immediate, OpBuilder, SourceSpan};
    let mut panics = vec![];
    for mutation in 0..4 {
        for (f, _) in fixtures() {
            let mut op = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            let count = op.borrow().operands().len();
            if mutation < 3 {
                if mutation >= count {
                    continue;
                }
                op.borrow_mut().operands_mut().group_mut(0).erase(mutation);
            } else {
                let mut builder = OpBuilder::new(f.context.clone());
                builder.set_insertion_point_before(op);
                let constant = builder.create::<crate::arith::Constant, _>(SourceSpan::UNKNOWN)(
                    Immediate::U32(7),
                )
                .unwrap();
                let value = constant.borrow().result().as_value_ref();
                op.borrow_mut().operands_mut().push(f.context.make_operand(value, op, 0));
            }
            let name = op.borrow().name().to_string();
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                select_function(&f.borrow(), &SelectEnv::default())
            })) {
                Ok(result) => assert!(result.is_err(), "{name}, operand mutation {mutation}"),
                Err(_) => panics.push(alloc::format!("{name}, operand mutation {mutation}")),
            }
        }
    }
    assert!(
        panics.is_empty(),
        "malformed source panicked instead of returning a diagnostic: {panics:?}"
    );
}

#[test]
fn memory_counts_and_copy_pointer_types_are_exact() {
    use midenc_hir::{PointerType, Type};
    let pointer = Type::from(PointerType::new(Type::U32));
    let mut fill = parse(
        "builtin.function public extern(\"C\") @fill(%p: ptr<u32, byte>, %n: u32, %v: u32) { \
         hir.mem_set %p, %n, %v : (ptr<u32, byte>,u32,u32); builtin.ret; };",
    );
    retype_parameters(&mut fill, &[pointer.clone(), Type::I32, Type::U32]);
    assert!(select_function(&fill.borrow(), &SelectEnv::default()).is_err());
    for op in ["mem_cpy", "mem_move"] {
        for wrong_count in [true, false] {
            let mut f = parse(&alloc::format!(
                "builtin.function public extern(\"C\") @copy(%p: ptr<u32, byte>, %q: ptr<u32, \
                 byte>, %n: u32) {{ hir.{op} %p, %q, %n : (ptr<u32, byte>,ptr<u32, byte>,u32); \
                 builtin.ret; }};"
            ));
            let types = if wrong_count {
                [pointer.clone(), pointer.clone(), Type::I32]
            } else {
                [pointer.clone(), Type::from(PointerType::new(Type::I32)), Type::U32]
            };
            retype_parameters(&mut f, &types);
            assert!(
                select_function(&f.borrow(), &SelectEnv::default()).is_err(),
                "{op}, wrong_count={wrong_count}"
            );
        }
    }
}

#[test]
fn shared_memory_and_arithmetic_result_verifier_diagnoses_malformed_results() {
    use midenc_hir::{Operation, Type, Value, Verify, traits::SameOperandsAndResultType};
    for mutation in 0..4 {
        let f = parse(
            "builtin.function public extern(\"C\") @arithmetic(%a: u32, %b: u32) { %r = arith.add \
             %a, %b <{ overflow = #builtin.overflow<wrapping> }>; builtin.ret; };",
        );
        let mut op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        match mutation {
            1 => {
                op.borrow_mut().results_mut().group_mut(0).pop().unwrap();
            }
            2 => {
                let mut result = op.borrow().results().all()[0];
                result.borrow_mut().set_type(Type::I32);
            }
            3 => {
                op.borrow_mut().operands_mut().clear();
            }
            _ => {}
        }
        let result =
            <Operation as Verify<dyn SameOperandsAndResultType>>::verify(&op.borrow(), &f.context);
        assert_eq!(result.is_ok(), matches!(mutation, 0 | 3), "mutation {mutation}");
        if mutation == 1 {
            assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
        }
    }
}

#[test]
fn byte_local_address_forms_select_and_replay() {
    for name in ["alloca", "local_byte_address"] {
        let mut f = parse(&alloc::format!(
            "builtin.function public extern(\"sigma\") @test() -> ptr<u32, byte> {{ %r = \
             hir.{name} <{{ local = #builtin.local_variable<0, u32> }}>; builtin.ret %r : \
             (ptr<u32, byte>); }};"
        ));
        f.function.borrow_mut().alloc_local(midenc_hir::Type::U32);
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        verify::verify(&plan, &frame, body).unwrap();
    }
}

fn unit_environment(f: &Parsed) -> SelectEnv<'static> {
    SelectEnv {
        byte_addressing: crate::lower::unit::UnitFacts::derive(f.borrow().as_operation(), &[])
            .byte_addressing,
        ..Default::default()
    }
}

#[test]
fn byte_local_store_and_load_use_derived_wide_policy() {
    for name in ["alloca", "local_byte_address"] {
        let mut f = parse(&alloc::format!(
            "builtin.function private extern(\"sigma\") @test(%v: u32) -> u32 {{ %p = hir.{name} \
             <{{ local = #builtin.local_variable<0, u32> }}>; hir.store %p, %v : (ptr<u32, byte>, \
             u32); %r = hir.load %p; builtin.ret %r : (u32); }};"
        ));
        f.function.borrow_mut().alloc_local(midenc_hir::Type::U32);
        let env = unit_environment(&f);
        assert_eq!(env.byte_addressing, crate::emit::addressing::ByteAddressing::Wide);
        execute_linked_memory_env(&f, &env, &[0xabcdef12], &[0xabcdef12], "", "", |_| {}).unwrap();
    }
}

#[test]
fn retyped_pointer_memory_loads_a_wide_byte_address() {
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{
        AddressSpace, PointerType, SourceSpan, Symbol, Type, ValueRef, Visibility,
        dialects::builtin::BuiltinOpBuilder, testing::Test,
    };
    for source in [None, Some(AddressSpace::Byte), Some(AddressSpace::Element)] {
        for destination in [AddressSpace::Byte, AddressSpace::Element] {
            let pointer =
                |pointee, space| Type::from(PointerType::new_with_address_space(pointee, space));
            let input_type = source.map_or(Type::U32, |space| pointer(Type::U32, space));
            let mut test = Test::new("pointer_memory", &[input_type], &[Type::U32]);
            test.function().borrow_mut().set_visibility(Visibility::Private);
            {
                let mut builder = test.function_builder();
                let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
                let target = pointer(pointer(Type::U32, AddressSpace::Byte), destination);
                let retyped = if source.is_some() {
                    builder.bitcast(input, target, SourceSpan::UNKNOWN).unwrap()
                } else {
                    builder.inttoptr(input, target, SourceSpan::UNKNOWN).unwrap()
                };
                let wide = builder.load(retyped, SourceSpan::UNKNOWN).unwrap();
                let value = builder.load(wide, SourceSpan::UNKNOWN).unwrap();
                builder.ret([value], SourceSpan::UNKNOWN).unwrap();
            }
            let f = Parsed {
                context: test.context_rc(),
                function: test.function(),
            };
            let env = unit_environment(&f);
            // Transparent casts preserve the numeric base. The selected destination space
            // determines which cell contains the stored byte pointer, whose target is above 2^32.
            let input = if destination == AddressSpace::Byte {
                256
            } else {
                64
            };
            execute_linked_memory_env(
                &f,
                &env,
                &[input],
                &[123],
                "push.4294967296 mem_store.64 push.123 mem_store.1073741824",
                "",
                |_| {},
            )
            .unwrap();
        }
    }
}

pub(super) fn gep_fixture(
    space: midenc_hir::AddressSpace,
    element: midenc_hir::Type,
    index: usize,
    load: bool,
) -> Parsed {
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{
        ArrayType, PointerType, SourceSpan, Type, ValueRef, dialects::builtin::BuiltinOpBuilder,
        testing::Test,
    };
    let ptr = Type::from(PointerType::new_with_address_space(
        Type::from(ArrayType::new(element.clone(), 3)),
        space,
    ));
    let result = if load {
        element.clone()
    } else {
        Type::from(PointerType::new_with_address_space(element, space))
    };
    let mut test = Test::new("gep", &[ptr], &[result]);
    {
        let mut builder = test.function_builder();
        let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
        let addr = builder.getelementptr(input, &[index], SourceSpan::UNKNOWN).unwrap();
        let result = if load {
            builder.load(addr, SourceSpan::UNKNOWN).unwrap()
        } else {
            addr
        };
        builder.ret([result], SourceSpan::UNKNOWN).unwrap();
    }
    Parsed {
        context: test.context_rc(),
        function: test.function(),
    }
}

#[test]
fn gep_crosses_narrow_boundary_and_scales_element_offsets() {
    use midenc_hir::{AddressSpace, Type};
    let f = gep_fixture(AddressSpace::Byte, Type::U32, 1, true);
    let env = unit_environment(&f);
    assert_eq!(env.byte_addressing, crate::emit::addressing::ByteAddressing::Wide);
    execute_linked_memory_env(
        &f,
        &env,
        &[0xffff_fffc],
        &[123],
        "push.123 mem_store.1073741824",
        "",
        |_| {},
    )
    .unwrap();
    let f = gep_fixture(AddressSpace::Element, Type::U32, 2, true);
    execute_linked_memory_env(
        &f,
        &unit_environment(&f),
        &[64],
        &[456],
        "push.456 mem_store.66",
        "",
        |_| {},
    )
    .unwrap();
}

#[test]
fn zero_gep_alias_and_malformed_gep_contracts() {
    use midenc_hir::{AddressSpace, Type, Value};
    let f = gep_fixture(AddressSpace::Element, Type::U32, 0, false);
    let selected = select_function(&f.borrow(), &SelectEnv::default()).unwrap();
    assert!(
        selected
            .segments
            .iter()
            .flat_map(|s| &s.units)
            .all(|u| u.stack_inputs.is_empty())
    );
    execute(&f, &SelectEnv::default(), &[64], &[64]);
    let f = gep_fixture(AddressSpace::Element, Type::U8, 1, false);
    assert_eq!(
        unit_environment(&f).byte_addressing,
        crate::emit::addressing::ByteAddressing::Wide
    );
    assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    for index in [0, 1] {
        let f = gep_fixture(AddressSpace::Byte, Type::U32, index, false);
        f.borrow().as_operation().prewalk_all(|op| {
            if op.is::<crate::hir::GetElementPtr>() {
                let mut result = op.results()[0];
                result.borrow_mut().set_type(Type::U32);
            }
        });
        assert_eq!(
            unit_environment(&f).byte_addressing,
            crate::emit::addressing::ByteAddressing::Wide
        );
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
}

#[test]
fn aggregate_local_gep_reads_each_struct_field() {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{
        AddressSpace, SourceSpan, StructType, Type, dialects::builtin::BuiltinOpBuilder,
        testing::Test,
    };
    for space in [AddressSpace::Byte, AddressSpace::Element] {
        for field in 0..4 {
            let mut test = Test::new("fields", &[], &[Type::U32]);
            {
                let mut builder = test.function_builder();
                let local = builder.alloc_local(Type::from(StructType::new(vec![Type::U32; 4])));
                let base = if space == AddressSpace::Byte {
                    builder.local_byte_address(local, Default::default()).unwrap()
                } else {
                    builder.local_address(local, Default::default()).unwrap()
                };
                let mut fields = vec![];
                for index in 0..4 {
                    let addr = builder.getelementptr(base, &[index], SourceSpan::UNKNOWN).unwrap();
                    let value = builder.u32(100 + index as u32, SourceSpan::UNKNOWN);
                    builder.store(addr, value, SourceSpan::UNKNOWN).unwrap();
                    fields.push(addr);
                }
                let value = builder.load(fields[field], SourceSpan::UNKNOWN).unwrap();
                builder.ret([value], SourceSpan::UNKNOWN).unwrap();
            }
            let f = Parsed {
                context: test.context_rc(),
                function: test.function(),
            };
            execute_linked_memory_env(
                &f,
                &unit_environment(&f),
                &[],
                &[100 + field],
                "",
                "",
                |_| {},
            )
            .unwrap();
        }
    }
}

/// Exercise typed locals against independently addressed memory, including their padding and
/// adjacent locals. The returned raw cells make mutually compensating load/store bugs visible.
fn aggregate_local_memory(ty: midenc_hir::Type, initialize: usize, store: bool) -> Parsed {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        AddressSpace, Felt, PointerType, SourceSpan, Type, dialects::builtin::BuiltinOpBuilder,
        testing::Test,
    };

    use crate::hir::HirOpBuilder;

    let cells = ty.size_in_felts();
    let mut results = vec![ty.clone()];
    results.extend(vec![Type::Felt; cells + 2]);
    let mut test = Test::new("aggregate_local_memory", std::slice::from_ref(&ty), &results);
    {
        let span = SourceSpan::UNKNOWN;
        let mut builder = test.function_builder();
        let value = builder.entry_block().borrow().arguments()[0].borrow().as_value_ref();
        let prefix = builder.alloc_local(Type::Felt);
        let local = builder.alloc_local(ty);
        let suffix = builder.alloc_local(Type::Felt);
        let prefix_value = builder.felt(Felt::new(17).unwrap(), span);
        let suffix_value = builder.felt(Felt::new(29).unwrap(), span);
        builder.store_local(prefix, prefix_value, span).unwrap();
        builder.store_local(suffix, suffix_value, span).unwrap();
        let base = builder.local_address(local, span).unwrap();
        let base = builder.ptrtoint(base, Type::U32, span).unwrap();
        let pointer_ty =
            Type::from(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element));
        let mut addresses = vec![];
        for i in 0..cells {
            let offset = builder.u32(i as u32, span);
            let address = builder.add(base, offset, span).unwrap();
            let address = builder.inttoptr(address, pointer_ty.clone(), span).unwrap();
            let fill = builder.felt(Felt::new(initialize as u64).unwrap(), span);
            builder.store(address, fill, span).unwrap();
            addresses.push(address);
        }
        if store {
            builder.store_local(local, value, span).unwrap();
        }
        let mut values = vec![builder.load_local(local, span).unwrap()];
        for address in addresses {
            values.push(builder.load(address, span).unwrap());
        }
        values.push(builder.load_local(prefix, span).unwrap());
        values.push(builder.load_local(suffix, span).unwrap());
        builder.ret(values, span).unwrap();
    }
    Parsed {
        context: test.context_rc(),
        function: test.function(),
    }
}

#[test]
fn aggregate_locals_preserve_lane_order_padding_and_neighboring_locals() {
    use midenc_hir::{ArrayType, StructType, Type, TypeRepr};
    let cases = [
        (Type::from(StructType::new([Type::U32, Type::U32])), vec![11, 23], vec![11, 23]),
        (
            Type::from(StructType::new([Type::U8, Type::U32, Type::U8])),
            vec![0x12, 0x34567890, 0x56],
            vec![0xa5a5a512, 0x34567890, 0xa5a5a556],
        ),
        (
            Type::from(ArrayType::new(Type::U8, 7)),
            vec![0x12345678, 0xabcdef],
            vec![0x12345678, 0xa5abcdef],
        ),
        (
            Type::from(StructType::new_with_repr(
                TypeRepr::packed(1),
                [Type::U8, Type::U64, Type::U8],
            )),
            vec![0x12345678, 0x9abcdef0, 0x4321],
            vec![0x12345678, 0x9abcdef0, 0xa5a54321],
        ),
        (
            Type::from(StructType::new_with_repr(TypeRepr::align(16), [Type::U64, Type::U8])),
            vec![0x12345678, 0x9abcdef0, 0x43],
            vec![0x12345678, 0x9abcdef0, 0xa5a5a543, 0xa5a5a5a5],
        ),
        (
            Type::from(StructType::new([Type::Felt, Type::U32])),
            vec![0xffff_ffff_0000_0000, 23],
            vec![0xffff_ffff_0000_0000, 23],
        ),
        (
            Type::from(StructType::new([Type::from(StructType::new([Type::Felt])), Type::U32])),
            vec![0xffff_ffff_0000_0000, 23],
            vec![0xffff_ffff_0000_0000, 23],
        ),
        (
            Type::from(ArrayType::new(Type::Felt, 4)),
            vec![0xffff_ffff_0000_0000, 11, 23, 37],
            vec![0xffff_ffff_0000_0000, 11, 23, 37],
        ),
    ];
    for (ty, input, cells) in cases {
        assert_eq!(ty.size_in_felts(), cells.len(), "raw cell expectation for {ty}");
        let f = aggregate_local_memory(ty.clone(), 0xa5a5a5a5, true);
        let expected = input.iter().copied().chain(cells).chain([17, 29]).collect::<Vec<_>>();
        execute_linked_memory(&f, &input, &expected, "", "")
            .unwrap_or_else(|error| panic!("aggregate local {ty}: {error:?}"));
    }
}

#[test]
fn aggregate_local_partial_stores_accept_full_field_frame_contents() {
    use midenc_hir::{ArrayType, Type};
    let f = aggregate_local_memory(Type::from(ArrayType::new(Type::U8, 3)), 0x1a5a5a5a5, true);
    execute_linked(&f, &[0x123456], &[0x123456, 0xa5123456, 17, 29]);
}

#[test]
fn aggregate_local_loads_observe_aliased_memory() {
    use midenc_hir::{StructType, Type};
    let fill = 0x1a5a5a543;
    let f =
        aggregate_local_memory(Type::from(StructType::new([Type::Felt, Type::U8])), fill, false);
    execute_linked(&f, &[0, 0], &[fill, 0x43, fill, fill, 17, 29]);
}

#[test]
fn aggregate_local_accesses_reject_zero_and_oversized_values() {
    use midenc_hir::{ArrayType, Type, testing::Test};

    use crate::hir::HirOpBuilder;
    for ty in [
        Type::from(ArrayType::new(Type::U8, 0)),
        Type::from(ArrayType::new(Type::U32, 5)),
    ] {
        for store in [false, true] {
            let params = if store { vec![ty.clone()] } else { vec![] };
            let mut test = Test::new("invalid_local_value", &params, &[]);
            {
                let mut builder = test.function_builder();
                let local = builder.alloc_local(ty.clone());
                if store {
                    let value =
                        builder.entry_block().borrow().arguments()[0].borrow().as_value_ref();
                    builder.store_local(local, value, Default::default()).unwrap();
                } else {
                    builder.load_local(local, Default::default()).unwrap();
                }
                use midenc_hir::dialects::builtin::BuiltinOpBuilder;
                builder.ret([], Default::default()).unwrap();
            }
            let error =
                select_function(&test.function().borrow(), &SelectEnv::default()).unwrap_err();
            assert!(format!("{error:?}").contains("one to four raw lanes"), "{ty}: {error:?}");
        }
    }
}

#[test]
fn aggregate_local_accesses_preserve_spill_sentinels() {
    use midenc_hir::{StructType, Type, dialects::builtin::BuiltinOpBuilder, testing::Test};

    use crate::hir::HirOpBuilder;
    let ty = Type::from(StructType::new(vec![Type::U32; 4]));
    let mut types = vec![Type::U32; 20];
    types.push(ty.clone());
    let mut test = Test::new("aggregate_pressure", &types, &types);
    {
        let mut builder = test.function_builder();
        let mut values = builder
            .entry_block()
            .borrow()
            .arguments()
            .iter()
            .map(|v| v.borrow().as_value_ref())
            .collect::<Vec<_>>();
        builder.alloc_local(Type::U128);
        let local = builder.alloc_local(ty);
        builder.store_local(local, values[20], Default::default()).unwrap();
        values[20] = builder.load_local(local, Default::default()).unwrap();
        builder.ret(values, Default::default()).unwrap();
    }
    let f = Parsed {
        context: test.context_rc(),
        function: test.function(),
    };
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    assert!(!frame.spills().is_empty());
    verify::verify(&plan, &frame, body).unwrap();
    let values = (100..120).chain([11, 23, 37, 41]).collect::<Vec<_>>();
    execute_linked(&f, &values, &values);
}
