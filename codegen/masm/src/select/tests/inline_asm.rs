use alloc::string::ToString;

use super::*;

#[test]
fn inline_assembly_replays_its_actual_stack_transition() {
    let f = parse(
        r#"builtin.function public extern("C") @asm(%0: felt) -> felt {
        %1 = hir.inline_asm "push.1 add" (%0) : extern("C") (felt) -> felt;
        builtin.ret %1 : (felt);
    };"#,
    );
    execute(&f, &SelectEnv::default(), &[41], &[42]);
}

pub(super) fn fixture(body: &str) -> Parsed {
    parse(&alloc::format!(
        r#"builtin.function public extern("C") @asm(%0: felt) -> felt {{
        %1 = hir.inline_asm {body:?} (%0) : extern("C") (felt) -> felt;
        builtin.ret %1 : (felt);
    }};"#
    ))
}

#[test]
fn inline_assembly_wrong_transition_fails_replay() {
    let f = fixture("push.1");
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    assert!(verify::verify(&plan, &frame, body).is_err());
}

#[test]
fn inline_assembly_empty_repuns_change_value_grouping() {
    let f = parse(
        r#"builtin.function public extern("C") @asm(%0: u64) -> (u32, u32) {
        %1, %2 = hir.inline_asm "" (%0) : extern("C") (u64) -> (u32, u32);
        builtin.ret %1, %2 : (u32, u32);
    };"#,
    );
    execute(&f, &SelectEnv::default(), &[3, 7], &[3, 7]);
    let selected = select_function(&f.borrow(), &SelectEnv::default()).unwrap();
    assert_eq!(selected.segments[0].units[0].publication, OutputPublication::Aliases);
    for (arguments, params, results) in [("%0", "felt", "u64"), ("", "", "felt")] {
        let f = parse(&alloc::format!(
            r#"builtin.function public extern("C") @bad(%0: felt) {{
            %1 = hir.inline_asm "" ({arguments}) : extern("C") ({params}) -> {results};
            builtin.ret;
        }};"#
        ));
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
    let f = parse(
        r#"builtin.function public extern("C") @empty() {
        hir.inline_asm "" () : extern("C") () -> (); builtin.ret;
    };"#,
    );
    execute(&f, &SelectEnv::default(), &[], &[]);
}

#[test]
fn inline_assembly_rejects_forbidden_instruction_families_and_wrapper_injection() {
    for body in [
        "locaddr.0",
        "loc_load.0",
        "loc_loadw_le.0",
        "loc_loadw_be.0",
        "loc_store.0",
        "loc_storew_le.0",
        "loc_storew_be.0",
        "sdepth",
        "dynexec",
        "dyncall",
        "emit",
        "emit.42",
        "trace",
        "trace.42",
        "while.true push.0 end",
        "repeat.2 nop end",
        "do nop while push.0 end",
        "push.UNKNOWN",
        "end proc injected nop",
        "end pub proc injected nop",
        "end const X = 0 proc injected nop",
        "end use ::external::callee proc injected nop",
    ] {
        let f = fixture(body);
        let result = select_function(&f.borrow(), &SelectEnv::default());
        let error = result.expect_err(&format!("accepted forbidden source: {body}"));
        let diagnostic = format!("{error:?}");
        let expected = if body.starts_with("loc") {
            "local"
        } else if body == "sdepth" {
            "sdepth"
        } else if body.starts_with("dyn") {
            "dynamic"
        } else if body.starts_with("emit") {
            "emit"
        } else if body.starts_with("trace") {
            "trace"
        } else if body.starts_with("while") || body.starts_with("repeat") || body.starts_with("do ")
        {
            "loops"
        } else if body == "push.UNKNOWN" {
            "UNKNOWN"
        } else {
            "wrapper"
        };
        assert!(
            diagnostic.contains(expected) || expected == "wrapper" && diagnostic.contains("syntax"),
            "{body}: {diagnostic}"
        );
    }
}

#[test]
fn inline_assembly_resolves_hir_callees_and_preserves_internal_branch_paths() {
    let f = parse_module_function(
        r#"builtin.module public @inline_test {
        builtin.function public extern("C") @callee(%x: felt) -> felt { builtin.ret %x : (felt); };
        builtin.function public extern("C") @dispatch(%condition: felt, %x: felt) -> felt {
            %r = hir.inline_asm "if.true exec.callee else push.1 add end" (%condition, %x) : extern("C") (felt, felt) -> felt;
            builtin.ret %r : (felt);
        };
    };"#,
    );
    let env = SelectEnv::default();
    let selected = selected_procedure(&f, &env).replace("::inline_test::callee", "callee");
    for (condition, result) in [(0, 43), (1, 42)] {
        execute_code(&alloc::format!(
            "proc callee nop end {selected} begin push.42 push.{condition} exec.selected \
             push.{result} assert_eq end"
        ))
        .unwrap();
    }
    let (plan, frame, mut body) = ready(&f, &env);
    fn mutate(body: &mut AnnotatedMasmBody) -> bool {
        for op in body.ops_mut() {
            if let AnnotatedOp::If {
                then_body,
                else_body,
                ..
            } = op
            {
                core::mem::swap(then_body, else_body);
                return true;
            }
        }
        false
    }
    assert!(mutate(&mut body));
    assert!(verify::verify(&plan, &frame, body).is_err());
}

#[test]
fn inline_assembly_calls_actual_typed_link_modules_and_intrinsics() {
    let session = crate::linked_callables::tests::session(&[(
        "::inline_linked",
        "pub proc add_one(x: felt) -> felt\n push.1 add\nend\npub proc untyped\n nop\nend",
    )]);
    let base =
        crate::linked_callables::SessionLinkInputs::load(&session, core::iter::empty::<&str>())
            .unwrap();
    let metadata = crate::linked_callables::LinkedCallables::from_link_inputs(&base, &[]).unwrap();
    let env = SelectEnv {
        linked_callables: Some(&metadata),
        ..Default::default()
    };
    let f = fixture("exec.::inline_linked::add_one");
    let procedure = selected_procedure(&f, &env);
    let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone());
    base.apply(&mut assembler).unwrap();
    let program = assembler
        .assemble_program(
            "inline-linked",
            alloc::format!("{procedure} begin push.41 exec.selected push.42 assert_eq end"),
        )
        .unwrap()
        .try_into_program()
        .unwrap();
    execute_program(&program, &mut crate::plan::tests::lowering_units::host(), &[]).unwrap();
    for callee in ["::inline_linked::untyped", "::missing::callee"] {
        let f = fixture(&alloc::format!("exec.{callee}"));
        assert!(
            select_function(&f.borrow(), &env)
                .unwrap_err()
                .to_string()
                .contains("no linked signature")
        );
    }
    let f = parse(
        r#"builtin.function public extern("C") @heap() -> u32 {
        %p = hir.inline_asm "exec.::intrinsics::mem::heap_base" () : extern("C") () -> u32;
        builtin.ret %p : (u32);
    };"#,
    );
    let (plan, frame, body) = ready(&f, &env);
    verify::verify(&plan, &frame, body).unwrap();
}

fn context_fixture(body: &str) -> Parsed {
    parse_module_function(&alloc::format!(
        r#"builtin.module public @inline_context {{
        builtin.function public extern("component-model") @callee(%x: felt) -> felt {{ builtin.ret %x : (felt); }};
        builtin.function public extern("C") @dispatch(%x: felt) -> felt {{
            %r = hir.inline_asm {body:?} (%x) : extern("C") (felt) -> felt;
            builtin.ret %r : (felt);
        }};
    }};"#
    ))
}

#[test]
fn inline_assembly_context_calls_require_explicit_window_padding_and_cleanup() {
    let pad = "push.0 swap.1 ".repeat(15);
    let cleanup = "swap.1 drop ".repeat(15);
    let body = alloc::format!("{pad} call.::inline_context::callee {cleanup}");
    let f = context_fixture(&body);
    let procedure =
        selected_procedure(&f, &SelectEnv::default()).replace("::inline_context::callee", "callee");
    execute_code(&alloc::format!(
        "proc callee nop end {procedure} begin push.901 push.42 exec.selected push.42 assert_eq \
         push.901 assert_eq end"
    ))
    .unwrap();
    for body in [
        "call.::inline_context::callee".to_string(),
        alloc::format!("{pad} call.::inline_context::callee"),
        alloc::format!("call.::inline_context::callee {cleanup}"),
    ] {
        let f = context_fixture(&body);
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        assert!(verify::verify(&plan, &frame, body).is_err());
    }
    for family in ["exec", "syscall"] {
        let f = context_fixture(&alloc::format!("{family}.::inline_context::callee"));
        let error = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
        assert!(
            family == "syscall" || error.to_string().contains("invocation family"),
            "{error:?}"
        );
    }
}

#[test]
fn inline_assembly_replay_rejects_target_family_and_early_publication_mutations() {
    let f = context_fixture(&alloc::format!(
        "{} call.::inline_context::callee {}",
        "push.0 swap.1 ".repeat(15),
        "swap.1 drop ".repeat(15)
    ));
    for mutation in 0..3 {
        let (plan, frame, mut body) = ready(&f, &SelectEnv::default());
        let site = body
            .ops()
            .iter()
            .position(|op| {
                matches!(
                    op,
                    AnnotatedOp::Instruction {
                        instruction: crate::masm::Instruction::Call(_),
                        ..
                    }
                )
            })
            .unwrap();
        match mutation {
            0 => {
                if let AnnotatedOp::Instruction {
                    instruction: crate::masm::Instruction::Call(target),
                    ..
                } = &mut body.ops_mut()[site]
                {
                    *target = crate::lower::raw::target("::inline_context::different");
                }
            }
            1 => {
                if let AnnotatedOp::Instruction { instruction, .. } = &mut body.ops_mut()[site] {
                    let crate::masm::Instruction::Call(target) = instruction else {
                        unreachable!()
                    };
                    *instruction = crate::masm::Instruction::Exec(target.clone());
                }
            }
            2 => {
                let end = body
                    .ops()
                    .iter()
                    .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitEnd(_))))
                    .unwrap();
                let marker = body.ops_mut().remove(end);
                body.ops_mut().insert(site, marker);
            }
            _ => unreachable!(),
        }
        assert!(verify::verify(&plan, &frame, body).is_err());
    }
}

#[test]
fn inline_assembly_rejects_canonical_hir_truncate_aliases() {
    for target in ["::miden::core::sys::truncate_stack", "arbitrary_alias"] {
        let f = parse_module_function(&alloc::format!(
            r#"builtin.module public @miden {{
            builtin.module public @core {{ builtin.module public @sys {{
                builtin.function public extern("C") @truncate_stack() {{ builtin.ret; }};
            }}; }};
            builtin.function_alias public @arbitrary_alias -> @core::@sys::@truncate_stack;
            builtin.function public extern("C") @dispatch() {{
                hir.inline_asm "exec.{target}" () : extern("C") () -> ();
                builtin.ret;
            }};
        }};"#
        ));
        let error = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
        assert!(error.to_string().contains("truncate_stack is forbidden"), "{error:?}");
    }
}

#[test]
fn inline_assembly_typed_kernel_syscall_executes_and_rejects_wrong_families() {
    let session = crate::linked_callables::tests::session(&[]);
    let kernel = crate::linked_callables::package_tests::package(
        &session,
        "::$kernel",
        "pub proc identity(x: felt) -> felt\n nop\nend",
        true,
    );
    let base =
        crate::linked_callables::SessionLinkInputs::load(&session, core::iter::empty::<&str>())
            .unwrap();
    let metadata = crate::linked_callables::LinkedCallables::from_link_inputs(
        &base,
        &[crate::linked_callables::LinkedPackageInput {
            package: kernel.clone(),
            linkage: miden_assembly::Linkage::Dynamic,
        }],
    )
    .unwrap();
    let env = SelectEnv {
        linked_callables: Some(&metadata),
        ..Default::default()
    };
    let f = fixture(&format!(
        "{} syscall.identity {}",
        "push.0 swap.1 ".repeat(15),
        "swap.1 drop ".repeat(15)
    ));
    // The upstream parser canonicalizes `syscall.identity` to an absolute AST target,
    // but does not accept its own absolute pretty-printing when reparsing this test text.
    let procedure =
        selected_procedure(&f, &env).replace("syscall.::$kernel::identity", "syscall.identity");
    let mut assembler =
        miden_assembly::Assembler::with_kernel(session.source_manager.clone(), kernel.clone())
            .unwrap();
    base.apply(&mut assembler).unwrap();
    let program = assembler
        .assemble_program(
            "inline-kernel",
            format!(
                "{procedure} begin push.901 push.42 exec.selected push.42 assert_eq push.901 \
                 assert_eq end"
            ),
        )
        .unwrap()
        .try_into_program()
        .unwrap();
    let mut host = crate::plan::tests::lowering_units::host();
    host.load_library(miden_processor::HostLibrary::from(kernel)).unwrap();
    execute_program(&program, &mut host, &[]).unwrap();
    for family in ["exec", "call"] {
        let f = fixture(&format!("{family}.::$kernel::identity"));
        let error = select_function(&f.borrow(), &env).unwrap_err();
        assert!(error.to_string().contains("invocation family"), "{error:?}");
    }
}

#[test]
fn inline_assembly_enforces_named_callee_visibility() {
    for visibility in ["private", "public"] {
        let f = parse_module_function(&format!(
            r#"builtin.module public @visibility_test {{
            builtin.module public @other {{
                builtin.function {visibility} extern("C") @callee(%x: felt) -> felt {{ builtin.ret %x : (felt); }};
            }};
            builtin.function public extern("C") @dispatch(%x: felt) -> felt {{
                %r = hir.inline_asm "exec.::visibility_test::other::callee" (%x) : extern("C") (felt) -> felt;
                builtin.ret %r : (felt);
            }};
        }};"#
        ));
        let selected = select_function(&f.borrow(), &SelectEnv::default());
        if visibility == "private" {
            assert!(
                selected.is_err(),
                "private target became callable through wrapper scaffolding"
            );
            assert!(selected.unwrap_err().to_string().contains("private callee"));
        } else {
            selected.unwrap();
        }
    }
}

#[test]
fn inline_assembly_empty_aggregate_regrouping_preserves_vm_lane_order() {
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{
        SourceSpan, StructType, Type, ValueRef,
        dialects::builtin::{BuiltinOpBuilder, attributes::Signature},
        testing::Test,
    };
    // The HIR attribute type parser does not yet accept struct signatures, so use the typed builder.
    let ty = Type::from(StructType::new([Type::U64, Type::U32, Type::U32]));
    let mut test = Test::new(
        "regroup",
        core::slice::from_ref(&ty),
        &[Type::U32, Type::U32, Type::U32, Type::U32],
    );
    let context = test.context_rc();
    let signature = Signature::new(&context, [ty], [Type::U32, Type::U32, Type::U32, Type::U32]);
    let mut builder = test.function_builder();
    let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
    let asm = builder.inline_asm("", signature, [input], SourceSpan::UNKNOWN).unwrap();
    let outputs = asm
        .borrow()
        .results()
        .iter()
        .map(|value| *value as ValueRef)
        .collect::<Vec<_>>();
    builder.ret(outputs, SourceSpan::UNKNOWN).unwrap();
    let f = Parsed {
        context,
        function: test.function(),
    };
    execute(&f, &SelectEnv::default(), &[11, 22, 33, 44], &[11, 22, 33, 44]);
}
