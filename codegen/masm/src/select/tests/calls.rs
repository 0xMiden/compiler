use midenc_hir::Value;

use super::*;

pub(super) fn direct(operation: &str, convention: &str) -> Parsed {
    use midenc_hir::{Builder, BuilderExt, OpBuilder, SourceSpan, ValueRef};
    let f = parse_module_function(&alloc::format!(
        "builtin.module public @direct_test {{
            builtin.function public extern(\"{convention}\") @callee(%x: felt) -> felt {{ \
         builtin.ret %x : (felt); }};
            builtin.function public extern(\"C\") @dispatch(%x: felt) -> felt {{ builtin.ret %x : \
         (felt); }};
        }};"
    ));
    let module = f.borrow().as_operation().nearest_symbol_table().unwrap();
    let callee = module
        .borrow()
        .downcast_ref::<builtin::Module>()
        .unwrap()
        .body()
        .entry()
        .body()
        .iter()
        .find_map(|op| op.as_operation_ref().try_downcast_op::<builtin::Function>().ok())
        .unwrap();
    let signature = (**callee.borrow().signature()).clone();
    let entry = f.borrow().entry_block();
    let argument = entry.borrow().arguments()[0] as ValueRef;
    let mut terminator = entry.borrow().terminator().unwrap();
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(terminator);
    let op = match operation {
        "exec" => builder.create::<crate::hir::Exec, (builtin::FunctionRef, _, [ValueRef; 1])>(
            SourceSpan::UNKNOWN,
        )(callee, signature, [argument])
        .unwrap()
        .as_operation_ref(),
        "call" => builder.create::<crate::hir::Call, (builtin::FunctionRef, _, [ValueRef; 1])>(
            SourceSpan::UNKNOWN,
        )(callee, signature, [argument])
        .unwrap()
        .as_operation_ref(),
        "syscall" => builder
            .create::<crate::hir::Syscall, (builtin::FunctionRef, _, [ValueRef; 1])>(
                SourceSpan::UNKNOWN,
            )(callee, signature, [argument])
        .unwrap()
        .as_operation_ref(),
        _ => unreachable!(),
    };
    let result = op.borrow().results().all()[0].borrow().as_value_ref();
    terminator.borrow_mut().operands_mut().group_mut(0)[0].borrow_mut().set(result);
    f
}

#[test]
fn direct_exec_preserves_unknown_addressable_and_overflow_suffix() {
    let f = direct("exec", "C");
    let procedure = selected_procedure(&f, &SelectEnv::default());
    assert!(procedure.contains("::direct_test::callee"));
    let procedure = procedure.replace("::direct_test::callee", "callee");
    let suffix = (901..925).collect::<Vec<_>>();
    let mut expected = vec![42];
    expected.extend(&suffix);
    execute_code(&alloc::format!(
        "proc callee nop end {procedure} begin {} push.42 exec.selected {} end",
        crate::plan::tests::lowering_units::push_top(&suffix),
        crate::plan::tests::lowering_units::assert_top(&expected),
    ))
    .unwrap();
}

#[test]
fn direct_context_call_uses_resolved_abi() {
    let f = direct("call", "component-model");
    let selected = checked(&f, &SelectEnv::default());
    assert_eq!(selected.segments[0].units.len(), 1);
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    verify::verify(&plan, &frame, body).unwrap();
}

#[test]
fn direct_invocation_family_mismatches_are_rejected() {
    for (operation, convention) in [("exec", "component-model"), ("call", "C"), ("syscall", "C")] {
        let f = direct(operation, convention);
        let err = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
        assert!(alloc::format!("{err:?}").contains("invocation family"), "{err:?}");
    }
}

fn truncate_fixture(target: &str, caller_cc: &str) -> Parsed {
    parse_module_function(&alloc::format!(
        r#"
        builtin.module public @miden {{
            builtin.module public @core {{ builtin.module public @sys {{
                builtin.function public extern("C") @truncate_stack() {{ builtin.ret; }};
            }}; }};
            builtin.function_alias public @arbitrary_alias -> @core::@sys::@truncate_stack;
            builtin.function public extern("{caller_cc}") @dispatch() {{
                hir.exec {target}() : extern("C") () -> ();
                builtin.ret;
            }};
        }};"#
    ))
}

#[test]
fn canonical_raw_and_aliased_truncate_require_registered_return_cleanup() {
    for target in [
        "::@miden::@core::@sys::@truncate_stack",
        "@core::@sys::@truncate_stack",
        "@arbitrary_alias",
    ] {
        for cc in ["C", "component-model"] {
            let f = truncate_fixture(target, cc);
            let err = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
            assert!(alloc::format!("{err:?}").contains("registered return cleanup"), "{err:?}");
        }
    }
}

#[test]
fn ordinary_same_named_procedure_retains_declared_suffix_contract() {
    let f = parse_module_function(
        r#"builtin.module public @user {
        builtin.function public extern("C") @truncate_stack() { builtin.ret; };
        builtin.function public extern("C") @dispatch() {
            hir.exec @truncate_stack() : extern("C") () -> (); builtin.ret;
        };
    };"#,
    );
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    verify::verify(&plan, &frame, body).unwrap();
}

#[test]
fn direct_result_types_follow_canonical_target() {
    let f = direct("exec", "C");
    let mut call = f
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .iter()
        .next()
        .unwrap()
        .as_operation_ref();
    call.borrow_mut().results_mut().group_mut(0)[0]
        .borrow_mut()
        .set_type(midenc_hir::Type::U32);
    let err = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
    assert!(alloc::format!("{err:?}").contains("direct results"), "{err:?}");
}

#[test]
fn semantic_fpi_groups_bind_distinct_suffix_prefix_root_and_payload() {
    use crate::lower::units::fpi::{Fpi, Operands};
    let a = |id| Atom::new(ValueId(id), 0);
    let payload = [a(6), a(7), a(2)];
    let outputs = (8..24).map(a).collect::<Vec<_>>();
    let (unit, proof) = Fpi
        .select_semantic(
            SelectedStepId(0),
            SourceStepId(0),
            Operands {
                account_suffix: a(0),
                account_prefix: a(1),
                procedure_root: [a(2), a(3), a(4), a(5)],
                payload: &payload,
            },
            &outputs,
        )
        .unwrap();
    assert_eq!(unit.stack_inputs, payload);
    assert_eq!(
        unit.stored_inputs.iter().map(|i| i.value).collect::<Vec<_>>(),
        (0..6).map(|i| StoredValue::Atom(a(i))).collect::<Vec<_>>()
    );
    assert_eq!(unit.semantic_inputs, (0..8).map(ValueId).collect::<Vec<_>>());
    assert_eq!(
        &proof.calls[0].inputs[..9],
        (0..6)
            .chain([6, 7, 2])
            .map(|i| verify::CallArgument::Exact(StoredValue::Atom(a(i))))
            .collect::<Vec<_>>()
    );
}

#[test]
fn legal_narrow_direct_extension_emitter_reproducer() {
    let f = parse("builtin.function public extern(\"C\") @f() { builtin.ret; };");
    let mut stack = crate::OperandStack::new(f.context.clone());
    stack.push(midenc_hir::Type::U8);
    let mut invoked = alloc::collections::BTreeSet::new();
    let mut body = vec![];
    crate::emit::OpEmitter::new(&mut invoked, &mut body, &mut stack)
        .zext(&midenc_hir::Type::U16, midenc_hir::SourceSpan::UNKNOWN);
}

#[test]
fn exact_kernel_owner_sets_entry_and_target_conventions() {
    for operation in ["exec", "call", "syscall"] {
        let f = direct(operation, "C");
        let mut owner = f.borrow().as_operation().nearest_symbol_table().unwrap();
        owner
            .borrow_mut()
            .downcast_mut::<builtin::Module>()
            .unwrap()
            .set_name(midenc_hir::Ident::from("$kernel"));
        let result = select_function(&f.borrow(), &SelectEnv::default());
        if operation == "syscall" {
            let selected = result.unwrap();
            assert_eq!(selected.abi.convention(), CallingConvention::Kernel);
            let (plan, frame, body) = ready(&f, &SelectEnv::default());
            verify::verify(&plan, &frame, body).unwrap();
        } else {
            assert!(alloc::format!("{:?}", result.unwrap_err()).contains("invocation family"));
        }
    }
}

pub(super) fn fixtures() -> Vec<(Parsed, bool)> {
    let kernel = direct("syscall", "C");
    let mut owner = kernel.borrow().as_operation().nearest_symbol_table().unwrap();
    owner
        .borrow_mut()
        .downcast_mut::<builtin::Module>()
        .unwrap()
        .set_name(midenc_hir::Ident::from("$kernel"));
    vec![
        (direct("exec", "C"), false),
        (direct("call", "component-model"), false),
        (kernel, false),
    ]
}

#[test]
fn legal_narrow_direct_signed_extension_emitter_reproducer() {
    let f = parse("builtin.function public extern(\"C\") @f() { builtin.ret; };");
    let mut stack = crate::OperandStack::new(f.context.clone());
    stack.push(midenc_hir::Type::I8);
    let mut invoked = alloc::collections::BTreeSet::new();
    let mut body = vec![];
    crate::emit::OpEmitter::new(&mut invoked, &mut body, &mut stack)
        .sext(&midenc_hir::Type::I16, midenc_hir::SourceSpan::UNKNOWN);
}

#[test]
fn unsigned_source_sext_preserves_existing_bitwise_semantics() {
    let f = parse("builtin.function public extern(\"C\") @f() { builtin.ret; };");
    let mut stack = crate::OperandStack::new(f.context.clone());
    stack.push(midenc_hir::Type::U8);
    let mut invoked = alloc::collections::BTreeSet::new();
    let mut body = vec![];
    crate::emit::OpEmitter::new(&mut invoked, &mut body, &mut stack)
        .sext(&midenc_hir::Type::I32, midenc_hir::SourceSpan::UNKNOWN);
    let code = body.iter().map(|op| op.to_pretty_string()).collect::<Vec<_>>().join(" ");
    execute_code(&alloc::format!(
        "begin push.901 push.255 {code} push.4294967295 assert_eq push.901 assert_eq end"
    ))
    .unwrap();
}

fn typed_direct(
    source: &str,
    target: &str,
    extension: builtin::attributes::ArgumentExtension,
) -> Parsed {
    use midenc_hir::{Builder, BuilderExt, OpBuilder, SourceSpan, ValueRef};
    let f = parse_module_function(&alloc::format!(
        r#"builtin.module public @typed {{
        builtin.function public extern("C") @callee(%x: {target}) -> {target} {{ builtin.ret %x : ({target}); }};
        builtin.function public extern("C") @dispatch(%x: {source}) -> ({target},{source}) {{
            %zero = arith.constant 0 : {target}; builtin.ret %zero,%x : ({target},{source});
        }};
    }};"#
    ));
    let module = f.borrow().as_operation().nearest_symbol_table().unwrap();
    let mut callee = module
        .borrow()
        .downcast_ref::<builtin::Module>()
        .unwrap()
        .body()
        .entry()
        .body()
        .iter()
        .find_map(|op| op.as_operation_ref().try_downcast_op::<builtin::Function>().ok())
        .unwrap();
    let ty = callee.borrow().signature().params()[0].ty.clone();
    use builtin::attributes::{AbiParam, ArgumentExtension};
    *callee.borrow_mut().get_signature_mut().param_mut(0).unwrap() = match extension {
        ArgumentExtension::Zext => AbiParam::zext(ty, &f.context),
        ArgumentExtension::Sext => AbiParam::sext(ty, &f.context),
        ArgumentExtension::None => AbiParam::new(ty),
    };
    let signature = (**callee.borrow().signature()).clone();
    let entry = f.borrow().entry_block();
    let argument = entry.borrow().arguments()[0] as ValueRef;
    let mut terminator = entry.borrow().terminator().unwrap();
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(terminator);
    let op =
        builder.create::<crate::hir::Exec, (builtin::FunctionRef, _, [ValueRef; 1])>(
            SourceSpan::UNKNOWN,
        )(callee, signature, [argument])
        .unwrap();
    let result = op.borrow().results().all()[0].borrow().as_value_ref();
    terminator.borrow_mut().operands_mut().group_mut(0)[0].borrow_mut().set(result);
    f
}

#[test]
fn direct_extension_phases_preserve_original_live_narrow_argument() {
    use builtin::attributes::ArgumentExtension::{Sext, Zext};
    for (src, dst, ext, input, result) in [
        ("i1", "u8", Zext, 1, 1),
        ("i1", "u16", Zext, 1, 1),
        ("i1", "i8", Sext, 1, 1),
        ("i1", "i16", Sext, 1, 1),
        ("u8", "u16", Zext, 255, 255),
        ("i8", "i16", Sext, 128, 65408),
        ("u8", "i16", Sext, 255, 65535),
        ("u8", "u32", Zext, 255, 255),
        ("u8", "i32", Sext, 255, u32::MAX as usize),
    ] {
        let f = typed_direct(src, dst, ext);
        let selected = checked(&f, &SelectEnv::default());
        assert_eq!(selected.segments.iter().map(|s| s.units.len()).sum::<usize>(), 3);
        let procedure =
            selected_procedure(&f, &SelectEnv::default()).replace("::typed::callee", "callee");
        execute_code(&alloc::format!(
            "proc callee nop end {procedure} begin push.902 push.901 push.{input} exec.selected \
             {} end",
            crate::plan::tests::lowering_units::assert_top(&[result, input, 901, 902])
        ))
        .unwrap_or_else(|error| panic!("{src}->{dst} {ext:?}: {error:?}\n{procedure}"));
    }
}

#[test]
fn direct_extension_source_phases_cannot_be_omitted_reordered_or_rebound() {
    let f = typed_direct("u8", "u64", builtin::attributes::ArgumentExtension::Zext);
    let function = f.borrow();
    let env = SelectEnv::default();
    let selected = select_function(&function, &env).unwrap();
    for mutation in 0..3 {
        let mut bad = selected.clone();
        let units = &mut bad.segments[0].units;
        match mutation {
            0 => {
                units.remove(1);
            }
            1 => units.swap(1, 2),
            _ => units[2].stack_inputs[0] = Atom::new(ValueId(0), 0),
        }
        assert!(
            check_selection(
                bad,
                &SourceContext::Hir {
                    function: &function,
                    env: &env
                }
            )
            .is_err()
        );
    }
}

#[test]
fn direct_extensions_reject_bad_width_sign_and_missing_attributes() {
    use builtin::attributes::ArgumentExtension::{None, Sext, Zext};
    for (src, dst, ext) in [
        ("u32", "u16", Zext),
        ("i8", "u32", Zext),
        ("u8", "i8", Sext),
        ("i32", "i16", Sext),
        ("u8", "u32", None),
    ] {
        let f = typed_direct(src, dst, ext);
        let err = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
        assert!(
            alloc::format!("{err:?}").contains("invalid direct argument extension"),
            "{err:?}"
        );
    }
}

#[test]
fn same_type_extension_attribute_is_a_noop() {
    let f = typed_direct("u8", "u8", builtin::attributes::ArgumentExtension::Zext);
    let selected = checked(&f, &SelectEnv::default());
    assert_eq!(selected.segments[0].units.len(), 2);
    let procedure =
        selected_procedure(&f, &SelectEnv::default()).replace("::typed::callee", "callee");
    execute_code(&alloc::format!(
        "proc callee nop end {procedure} begin push.902 push.901 push.255 exec.selected {} end",
        crate::plan::tests::lowering_units::assert_top(&[255, 255, 901, 902])
    ))
    .unwrap();
}

#[test]
fn nested_selected_exec_and_context_calls_preserve_live_after_input_and_suffix() {
    let outer = parse_module_function(
        r#"builtin.module public @outer {
        builtin.function public extern("C") @callee(%x: felt) -> felt { builtin.ret %x : (felt); };
        builtin.function public extern("C") @dispatch(%x: felt) -> (felt,felt) {
            %r=hir.exec @callee(%x) : extern("C") (felt) -> felt;
            builtin.ret %r,%x : (felt,felt);
        };
    };"#,
    );
    let inner = direct("call", "component-model");
    let outer =
        selected_procedure(&outer, &SelectEnv::default()).replace("::outer::callee", "inner");
    let inner = selected_procedure(&inner, &SelectEnv::default())
        .replace("proc selected", "proc inner")
        .replace("::direct_test::callee", "deep");
    let mut deep = String::from("proc deep push.100 swap.1 drop ");
    for i in 1..16 {
        if i == 1 {
            deep.push_str("swap.1 push.801 swap.1 drop swap.1 ");
        } else {
            deep.push_str(&alloc::format!("movup.{i} push.{} swap.1 drop movdn.{i} ", 800 + i));
        }
    }
    deep.push_str("end");
    let suffix = (901..925).collect::<Vec<_>>();
    let mut expected = vec![100, 42];
    expected.extend(&suffix);
    execute_code(&alloc::format!(
        "{deep} {inner} {outer} begin {} push.42 exec.selected {} end",
        crate::plan::tests::lowering_units::push_top(&suffix),
        crate::plan::tests::lowering_units::assert_top(&expected)
    ))
    .unwrap();
}

#[test]
fn direct_wide_extension_publishes_all_limbs_and_retains_original() {
    use builtin::attributes::ArgumentExtension::{Sext, Zext};
    for (src, dst, ext, input, expected) in [
        ("u8", "u64", Zext, vec![255], vec![255, 0, 255]),
        (
            "i8",
            "i64",
            Sext,
            vec![128],
            vec![u32::MAX as usize - 127, u32::MAX as usize, 128],
        ),
        ("u64", "u128", Zext, vec![17, 19], vec![17, 19, 0, 0, 17, 19]),
    ] {
        let f = typed_direct(src, dst, ext);
        let procedure =
            selected_procedure(&f, &SelectEnv::default()).replace("::typed::callee", "callee");
        let mut expected = expected;
        expected.extend([901, 902]);
        execute_code(&alloc::format!(
            "proc callee nop end {procedure} begin push.902 push.901 {} exec.selected {} end",
            crate::plan::tests::lowering_units::push_top(&input),
            crate::plan::tests::lowering_units::assert_top(&expected)
        ))
        .unwrap();
    }
}

#[test]
fn sret_requires_first_pointer_argument_and_no_direct_results() {
    for case in 0..4 {
        let (params, args, types, position, returns) = match case {
            0 => ("%p: ptr<u32, byte>", "%p", "ptr<u32, byte>", 0, false),
            1 => ("%p: u32", "%p", "u32", 0, false),
            2 => ("%x: felt,%p: ptr<u32, byte>", "%x,%p", "felt,ptr<u32, byte>", 1, false),
            _ => ("%p: ptr<u32, byte>", "%p", "ptr<u32, byte>", 0, true),
        };
        let result = if returns { " -> felt" } else { "" };
        let callee_body = if returns {
            "builtin.ret_imm 0 : felt;"
        } else {
            "builtin.ret;"
        };
        let call_result = if returns { "%r=" } else { "" };
        let result_type = if returns { "felt" } else { "()" };
        let f = parse_module_function(&alloc::format!(
            r#"builtin.module public @sret {{
            builtin.function public extern("C") @callee({params}){result} {{ {callee_body} }};
            builtin.function public extern("C") @dispatch({params}) {{
                {call_result}hir.exec @callee({args}) : extern("C") ({types}) -> {result_type}; builtin.ret;
            }};
        }};"#
        ));
        let module = f.borrow().as_operation().nearest_symbol_table().unwrap();
        let mut callee = module
            .borrow()
            .downcast_ref::<builtin::Module>()
            .unwrap()
            .body()
            .entry()
            .body()
            .iter()
            .find_map(|op| op.as_operation_ref().try_downcast_op::<builtin::Function>().ok())
            .unwrap();
        callee
            .borrow_mut()
            .get_signature_mut()
            .param_mut(position)
            .unwrap()
            .mark_sret(&f.context);
        let result = select_function(&f.borrow(), &SelectEnv::default());
        if case == 0 {
            let (plan, frame, body) = ready(&f, &SelectEnv::default());
            verify::verify(&plan, &frame, body).unwrap();
        } else {
            assert!(alloc::format!("{:?}", result.unwrap_err()).contains("sret requires"));
        }
    }
}

#[test]
fn alias_family_validation_uses_canonical_target_convention() {
    let f = parse_module_function(
        r#"builtin.module public @alias_test {
        builtin.function public extern("component-model") @callee(%x: felt) -> felt { builtin.ret %x : (felt); };
        builtin.function_alias public @ordinary_name -> @callee;
        builtin.function public extern("C") @dispatch(%x: felt) -> felt {
            %r=hir.exec @ordinary_name(%x) : extern("C") (felt) -> felt; builtin.ret %r : (felt);
        };
    };"#,
    );
    let error = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
    assert!(alloc::format!("{error:?}").contains("invocation family"), "{error:?}");
}

#[test]
fn kernel_descendant_module_is_not_a_kernel_procedure_owner() {
    let f = parse_module_function(
        r#"builtin.module public @"$kernel" {
        builtin.module public @ordinary {
            builtin.function public extern("C") @callee(%x: felt) -> felt { builtin.ret %x : (felt); };
        };
        builtin.function public extern("C") @dispatch(%x: felt) -> felt {
            %r=hir.exec @ordinary::@callee(%x) : extern("C") (felt) -> felt; builtin.ret %r : (felt);
        };
    };"#,
    );
    // Source selection checks the callee's convention before physical return normalization.
    checked(&f, &SelectEnv::default());
}

#[test]
fn source_direct_call_preserves_debugger_frames() {
    use alloc::sync::Arc;
    let f = direct("call", "component-model");
    let selected =
        selected_procedure(&f, &SelectEnv::default()).replace("::direct_test::callee", "callee");
    let manager = Arc::new(miden_assembly::DefaultSourceManager::default());
    let code =
        format!("proc callee nop end {selected} begin push.42 exec.selected push.42 assert_eq end");
    let package = miden_assembly::Assembler::new(manager.clone())
        .assemble_program("source_direct_call_debug_frames", code)
        .unwrap();
    crate::plan::tests::lowering_units::assert_debugger_call_frames(
        package.into(),
        manager,
        [],
        "selected",
    )
    .unwrap();
}
