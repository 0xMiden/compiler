use super::*;

#[test]
fn real_hir_indirect_later_phase_uses_and_contents_survive_staging() {
    let text = "builtin.function public extern(\"C\") @dispatch(%r0: felt,%r1: felt,%r2: \
                felt,%r3: felt) -> felt { %result=hir.dyncall [%r0,%r1,%r2,%r3](%r0) : \
                extern(\"component-model\") (felt) -> (felt); builtin.ret %result : (felt); };";
    let f = parse(text);
    let selected = checked(&f, &SelectEnv::default());
    assert_eq!(selected.segments.iter().map(|s| s.units.len()).sum::<usize>(), 2);
    let stage = &selected.segments[0].units[0];
    let dispatch = selected
        .segments
        .iter()
        .flat_map(|s| &s.units)
        .find(|u| u.id != stage.id)
        .unwrap();
    assert_eq!(stage.scratch[0].uses, [stage.id, dispatch.id]);
    assert_eq!(dispatch.scratch_effects.len(), 4);
    let analysis = crate::allocate::analysis::analyze(&selected);
    assert!(analysis.after[&stage.id].contains(stage.stack_inputs[0].value));
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    verify::verify(&plan, &frame, body).unwrap();
}

#[test]
fn real_hir_table_lookup_address_remains_live_until_dispatch() {
    let f = parse_module_function(include_str!("fixtures/indirect-table.hir"));
    let info = linked(&f);
    let env = SelectEnv {
        link_info: Some(&info),
        ..Default::default()
    };
    let selected = checked(&f, &env);
    let stage = &selected.segments[0].units[0];
    let address = stage.outputs[0];
    let analysis = crate::allocate::analysis::analyze(&selected);
    assert!(analysis.after[&stage.id].contains(address.value));
    let dispatch = &selected.segments[1].units[0];
    assert_eq!(dispatch.stored_inputs[0].value, StoredValue::Atom(address));
    let (plan, frame, body) = ready(&f, &env);
    verify::verify(&plan, &frame, body).unwrap();
}

fn table_code(index: usize, tag: u32) -> String {
    let f = parse_module_function(include_str!("fixtures/indirect-table.hir"));
    let info = linked(&f);
    let env = SelectEnv {
        link_info: Some(&info),
        ..Default::default()
    };
    let source = crate::select::evidence(&f.borrow(), &env).unwrap();
    let base = source
        .definitions
        .iter()
        .find_map(|definition| match &definition.body {
            crate::lower::units::HirBody::Indirect(
                crate::lower::units::calls::Indirect::Lookup { base, .. },
            ) => Some(*base),
            _ => None,
        })
        .unwrap();
    let (plan, frame, body) = ready(&f, &env);
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string();
    alloc::format!(
        "proc callee nop end {procedure} begin procref.callee mem_storew_le.{} dropw push.{tag} \
         mem_store.{} push.902 push.901 push.42 push.{index} exec.selected push.42 assert_eq \
         push.901 assert_eq push.902 assert_eq end",
        base + 8,
        base + 12
    )
}

#[test]
fn registered_table_lookup_executes_matching_slot_and_preserves_suffix() {
    execute_code(&table_code(1, 7)).unwrap();
}

#[test]
fn registered_table_lookup_traps_for_null_out_of_bounds_and_wrong_tag() {
    for (index, tag, message) in [
        (0, 7, "indirect call: callee signature mismatch or null function reference"),
        (2, 7, "indirect call: function table index out of bounds"),
        (1, 8, "indirect call: callee signature mismatch or null function reference"),
    ] {
        let error = execute_code(&table_code(index, tag)).unwrap_err();
        let text = alloc::format!("{error:?}");
        assert!(
            text.contains(message)
                || text.contains(&miden_core::mast::error_code_from_msg(message).to_string()),
            "{error:?}"
        );
    }
}

#[test]
fn checked_table_selector_rejects_wrong_consumed_address_token() {
    let f = parse_module_function(include_str!("fixtures/indirect-table.hir"));
    let info = linked(&f);
    let env = SelectEnv {
        link_info: Some(&info),
        ..Default::default()
    };
    let (plan, frame, mut body) = ready(&f, &env);
    let mut changed = false;
    crate::plan::tests::lowering_units::rewrite(&mut body, &mut |op| {
        if let AnnotatedOp::Instruction { instruction, .. } = op
            && matches!(instruction, I::LocLoad(_))
        {
            *instruction =
                Primitive::Push(miden_core::Felt::new_unchecked(123)).instruction().unwrap();
            changed = true;
        }
    });
    assert!(changed);
    assert_eq!(
        verify::verify(&plan, &frame, body).unwrap_err().category,
        DiagnosticCategory::AbiMismatch
    );
}

#[test]
fn real_hir_indirect_missing_phase_uses_are_rejected_by_source_check() {
    let f = parse_module_function(include_str!("fixtures/indirect-table.hir"));
    let info = linked(&f);
    let env = SelectEnv {
        link_info: Some(&info),
        ..Default::default()
    };
    let function = f.borrow();
    let selected = select_function(&function, &env).unwrap();
    let checked = check_selection(
        selected.clone(),
        &SourceContext::Hir {
            function: &function,
            env: &env,
        },
    )
    .unwrap();
    assert!(!crate::allocate::analysis::analyze(&checked).after.is_empty());
    for mutation in 0..4 {
        let mut bad = selected.clone();
        let unit = &mut bad.segments[1].units[0];
        match mutation {
            0 => unit.semantic_inputs.clear(),
            1 => unit.stored_inputs.clear(),
            2 => {
                bad.segments[0].units.clear();
            }
            _ => unit.outputs.clear(),
        };
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
    let dyncall = parse(
        "builtin.function public extern(\"C\") @dispatch(%r0: felt,%r1: felt,%r2: felt,%r3: felt) \
         { hir.dyncall [%r0,%r1,%r2,%r3]() : extern(\"component-model\") () -> (); builtin.ret; };",
    );
    let function = dyncall.borrow();
    let env = SelectEnv::default();
    let mut bad = select_function(&function, &env).unwrap();
    bad.segments[1].units[0].scratch_effects.clear();
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

use crate::masm::Instruction as I;

#[test]
fn real_hir_indirect_missing_phase_preservation_rejects_corrupted_allocator_liveness() {
    use crate::plan::actions::PlanAction;
    let f = parse_module_function(include_str!("fixtures/indirect-table.hir"));
    let info = linked(&f);
    let env = SelectEnv {
        link_info: Some(&info),
        ..Default::default()
    };
    let (mut plan, frame, mut body) = ready(&f, &env);
    let dispatch = plan.selected.segments[1].units[0].id;
    let slot = plan.lowering_bindings[&dispatch].stored_inputs[&StoredInputId(0)];
    let offset = frame.resolve(FrameRef::Spill(slot)).unwrap();
    let mut changed_actions = 0;
    // Model a wrong dead-after-lookup decision in the actual preservation plan: drop its
    // only stored copy and claim no incoming slot requirement, while keeping checked uses.
    for actions in plan.segment_actions.values_mut().chain(plan.edge_actions.values_mut()) {
        for action in actions {
            if *action == PlanAction::StoreLane(slot) {
                *action = PlanAction::Drop;
                changed_actions += 1;
            }
        }
    }
    for contract in plan.contracts.values_mut() {
        contract.required_spill_slots.retain(|(required, _)| *required != slot);
    }
    let mut changed_code = 0;
    crate::plan::tests::lowering_units::rewrite(&mut body, &mut |op| {
        if let AnnotatedOp::Instruction { instruction, unit } = op
            && *unit == SelectedStepId(u32::MAX)
            && *instruction == I::LocStore(offset.into())
        {
            *instruction = I::Drop;
            changed_code += 1;
        }
    });
    assert_eq!(changed_actions, changed_code);
    assert!(changed_actions > 0);
    let error = verify::verify(&plan, &frame, body).unwrap_err();
    assert!(
        matches!(
            error.category,
            DiagnosticCategory::InvalidSpillSlotRead | DiagnosticCategory::UnavailableValue
        ),
        "{error}"
    );
}

#[test]
fn a_real_before_region_exposes_its_internal_call_phase_boundary() {
    let f = parse(include_str!("fixtures/indirect-before.hir"));
    let env = SelectEnv::default();
    let selected = checked(&f, &env);
    let analysis = crate::allocate::analysis::analyze(&selected);
    let units = selected
        .segments
        .iter()
        .flat_map(|s| s.units.iter().map(move |u| (s.id, u)))
        .collect::<Vec<_>>();
    assert_eq!(units.len(), 2);
    assert_ne!(units[0].0, units[1].0);
    let demand = analysis.loops.values().next().unwrap();
    assert!(demand.members.contains(&units[0].0));
    assert!(demand.members.contains(&units[1].0));
    assert_eq!(analysis.scratch.after[&units[0].1.id].len(), 4);
    let (plan, frame, body) = ready(&f, &env);
    verify::verify(&plan, &frame, body).unwrap();
}

#[test]
fn sixteen_argument_dyncall_preserves_a_root_argument_used_after_the_call() {
    let params = (1..16).map(|i| alloc::format!("%a{i}: felt")).collect::<Vec<_>>().join(", ");
    let args = (1..16).map(|i| alloc::format!("%a{i}")).collect::<Vec<_>>().join(", ");
    let types = vec!["felt"; 16].join(", ");
    let text = alloc::format!(
        "builtin.function public extern(\"C\") @dispatch(%r0: felt,%r1: felt,%r2: felt,%r3: \
         felt,{params}) -> (felt,felt) {{ %result=hir.dyncall [%r0,%r1,%r2,%r3](%r0,{args}) : \
         extern(\"component-model\") ({types}) -> felt; builtin.ret %result,%r0 : (felt,felt); }};"
    );
    let f = parse(&text);
    let selected = checked(&f, &SelectEnv::default());
    let stage = &selected.segments[0].units[0];
    let dispatch = &selected.segments[1].units[0];
    assert_eq!(dispatch.stack_inputs.len(), 16);
    let analysis = crate::allocate::analysis::analyze(&selected);
    assert!(analysis.after[&dispatch.id].contains(stage.stack_inputs[0].value));
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string();
    let mut callee = String::from("proc callee ");
    for i in 1..16 {
        if i == 1 {
            callee.push_str("swap.1 push.801 swap.1 drop swap.1 ");
        } else {
            callee.push_str(&alloc::format!("movup.{i} push.{} swap.1 drop movdn.{i} ", 800 + i));
        }
    }
    callee.push_str("end");
    execute_code(&alloc::format!(
        "{callee} {procedure} begin push.902 push.901 {} procref.callee exec.selected dup.0 dup.2 \
         assert_eq drop drop push.901 assert_eq push.902 assert_eq end",
        crate::plan::tests::lowering_units::push_top(&(1..16).collect::<Vec<_>>()),
    ))
    .unwrap();
}

#[test]
fn sixteen_argument_table_exec_preserves_its_address_and_live_after_argument() {
    let params = (0..16).map(|i| alloc::format!("%a{i}: felt")).collect::<Vec<_>>().join(", ");
    let args = (0..16).map(|i| alloc::format!("%a{i}")).collect::<Vec<_>>().join(", ");
    let types = vec!["felt"; 16].join(", ");
    let text = alloc::format!(
        "builtin.module public @table_test {{ builtin.function private extern(\"C\") \
         @callee({params}) -> felt {{ builtin.ret %a0 : (felt); }}; builtin.function_table \
         private @table : 2 {{builtin.function_table_entry 1 @callee tag 7;}}; builtin.function \
         public extern(\"C\") @dispatch(%index: u32,{params}) -> (felt,felt) {{ \
         %result=hir.exec_indirect @table[%index]({args}) : extern(\"C\") ({types}) -> felt tag \
         7; builtin.ret %result,%a1 : (felt,felt); }}; }};"
    );
    let f = parse_module_function(&text);
    let info = linked(&f);
    let env = SelectEnv {
        link_info: Some(&info),
        ..Default::default()
    };
    let selected = checked(&f, &env);
    assert_eq!(selected.segments[1].units[0].stack_inputs.len(), 16);
    let evidence = crate::select::evidence(&f.borrow(), &env).unwrap();
    let base = evidence
        .definitions
        .iter()
        .find_map(|d| match d.body {
            crate::lower::units::HirBody::Indirect(
                crate::lower::units::calls::Indirect::Lookup { base, .. },
            ) => Some(base),
            _ => None,
        })
        .unwrap();
    let (plan, frame, body) = ready(&f, &env);
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("selected").unwrap())
        .unwrap()
        .to_pretty_string();
    let arguments = (1..=16).collect::<Vec<usize>>();
    let code = alloc::format!(
        "proc callee repeat.15 swap.1 drop end end {procedure} begin procref.callee \
         mem_storew_le.{} dropw push.7 mem_store.{} push.902 push.901 {} push.1 exec.selected {} \
         end",
        base + 8,
        base + 12,
        crate::plan::tests::lowering_units::push_top(&arguments),
        crate::plan::tests::lowering_units::assert_top(&[1, 2, 901, 902])
    );
    execute_code(&code).unwrap();
}

#[test]
fn indirect_results_must_match_declared_signature_types_and_arity() {
    use midenc_hir::{SourceSpan, Type, Value};
    for table in [false, true] {
        for mutation in 0..3 {
            let f = if table {
                parse_module_function(
                    &include_str!("fixtures/indirect-table.hir")
                        .replace("builtin.ret %result : (felt);", "builtin.ret %value : (felt);"),
                )
            } else {
                parse(
                    "builtin.function public extern(\"C\") @dispatch(%r0: felt,%r1: felt,%r2: \
                     felt,%r3: felt) { %unused=hir.dyncall [%r0,%r1,%r2,%r3]() : \
                     extern(\"component-model\") () -> felt; builtin.ret; };",
                )
            };
            let mut call = f
                .borrow()
                .entry_block()
                .borrow()
                .body()
                .front()
                .get()
                .unwrap()
                .as_operation_ref();
            if mutation < 2 {
                let mut result = call.borrow().results().all()[0];
                let ty = if mutation == 0 { Type::U64 } else { Type::U32 };
                result.borrow_mut().set_type(ty.clone());
                let info = table.then(|| linked(&f));
                let env = SelectEnv {
                    link_info: info.as_ref(),
                    ..Default::default()
                };
                assert_malformed_source_rejected(&f, &env);
                assert_eq!(result.borrow().ty(), &ty);
            } else {
                let extra = f.context.make_result(SourceSpan::UNKNOWN, Type::Felt, call, 1);
                call.borrow_mut().results_mut().push(extra);
                let info = table.then(|| linked(&f));
                let env = SelectEnv {
                    link_info: info.as_ref(),
                    ..Default::default()
                };
                assert_malformed_source_rejected(&f, &env);
                assert_eq!(call.borrow().num_results(), 2);
            }
        }
    }
}

#[test]
fn live_table_entry_cannot_hide_depth_sensitive_helper() {
    let f = parse_module_function(
        r#"builtin.module public @miden {
        builtin.module public @core { builtin.module public @sys {
            builtin.function public extern("C") @truncate_stack() { builtin.ret; };
        }; };
        builtin.function_alias public @arbitrary -> @core::@sys::@truncate_stack;
        builtin.function_table private @table : 1 { builtin.function_table_entry 0 @arbitrary tag 7; };
        builtin.function public extern("C") @dispatch(%i: u32) {
            hir.exec_indirect @table[%i]() : extern("C") () -> () tag 7; builtin.ret;
        };
    };"#,
    );
    let info = linked(&f);
    let error = select_function(
        &f.borrow(),
        &SelectEnv {
            link_info: Some(&info),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(alloc::format!("{error:?}").contains("registered return cleanup"), "{error:?}");
}

#[test]
fn live_table_entry_convention_follows_kernel_export_visibility() {
    for visibility in ["public", "internal", "private"] {
        let f = parse_module_function(
            &include_str!("fixtures/indirect-table.hir")
                .replace("builtin.function private", &format!("builtin.function {visibility}")),
        );
        let mut owner = f.borrow().as_operation().nearest_symbol_table().unwrap();
        owner
            .borrow_mut()
            .downcast_mut::<builtin::Module>()
            .unwrap()
            .set_name(midenc_hir::Ident::from("$kernel"));
        let info = linked(&f);
        let env = SelectEnv {
            link_info: Some(&info),
            ..Default::default()
        };
        let result = select_function(&f.borrow(), &env);
        if visibility != "public" {
            assert_eq!(result.unwrap().abi.convention(), CallingConvention::Kernel);
            let (plan, frame, body) = ready(&f, &env);
            verify::verify(&plan, &frame, body).unwrap();
        } else {
            let error = result.unwrap_err();
            assert!(
                alloc::format!("{error:?}").contains("invocation family"),
                "{visibility}: {error:?}"
            );
        }
    }
}

#[test]
fn indirect_same_type_extension_attributes_remain_noops_and_types_are_exact() {
    use builtin::attributes::AbiParam;
    use midenc_hir::Type;
    for table in [false, true] {
        let f = if table {
            parse_module_function(
                &include_str!("fixtures/indirect-table.hir").replace("felt", "u8"),
            )
        } else {
            parse(
                r#"builtin.function public extern("C") @dispatch(%r0: felt,%r1: felt,%r2: felt,%r3: felt,%x: u8) -> u8 {
                %r=hir.dyncall [%r0,%r1,%r2,%r3](%x) : extern("component-model") (u8) -> u8;
                builtin.ret %r : (u8);
            };"#,
            )
        };
        let call = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .iter()
            .next()
            .unwrap()
            .as_operation_ref();
        if table {
            let mut typed = call.try_downcast_op::<crate::hir::ExecIndirect>().unwrap();
            *typed.borrow_mut().get_signature_mut().param_mut(0).unwrap() =
                AbiParam::zext(Type::U8, &f.context);
            let module = f.borrow().as_operation().nearest_symbol_table().unwrap();
            let mut target = module
                .borrow()
                .downcast_ref::<builtin::Module>()
                .unwrap()
                .body()
                .entry()
                .body()
                .iter()
                .find_map(|op| op.as_operation_ref().try_downcast_op::<builtin::Function>().ok())
                .unwrap();
            *target.borrow_mut().get_signature_mut().param_mut(0).unwrap() =
                AbiParam::zext(Type::U8, &f.context);
        } else {
            let mut typed = call.try_downcast_op::<crate::hir::Dyncall>().unwrap();
            *typed.borrow_mut().get_signature_mut().param_mut(0).unwrap() =
                AbiParam::zext(Type::U8, &f.context);
        }
        let info = table.then(|| linked(&f));
        let env = SelectEnv {
            link_info: info.as_ref(),
            ..Default::default()
        };
        let (plan, frame, body) = ready(&f, &env);
        verify::verify(&plan, &frame, body).unwrap();
        let operand = if table { 1 } else { 4 };
        let mut argument = call.borrow().operands().all()[operand].borrow().as_value_ref();
        argument.borrow_mut().set_type(Type::U16);
        assert!(select_function(&f.borrow(), &env).is_err());
    }
}

#[test]
fn context_signature_bounds_exclude_only_actual_arguments_and_results() {
    for results in [false, true] {
        let types = vec!["felt"; 17].join(",");
        let args = vec!["%r0"; 17].join(",");
        let call = if results {
            alloc::format!(
                "%r:17=hir.dyncall [%r0,%r1,%r2,%r3]() : extern(\"component-model\") () -> \
                 ({types});"
            )
        } else {
            alloc::format!(
                "hir.dyncall [%r0,%r1,%r2,%r3]({args}) : extern(\"component-model\") ({types}) -> \
                 ();"
            )
        };
        let f = parse(&alloc::format!(
            "builtin.function public extern(\"C\") @dispatch(%r0: felt,%r1: felt,%r2: felt,%r3: \
             felt) {{ {call} builtin.ret; }};"
        ));
        let error = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
        assert!(alloc::format!("{error:?}").contains("sixteen-lane window"), "{error:?}");
    }
}

#[test]
fn table_matching_tag_requires_complete_convention() {
    let f = parse_module_function(include_str!("fixtures/indirect-table.hir"));
    let module = f.borrow().as_operation().nearest_symbol_table().unwrap();
    let mut target = module
        .borrow()
        .downcast_ref::<builtin::Module>()
        .unwrap()
        .body()
        .entry()
        .body()
        .iter()
        .find_map(|op| op.as_operation_ref().try_downcast_op::<builtin::Function>().ok())
        .unwrap();
    target.borrow_mut().get_signature_mut().cc = midenc_hir::CallConv::ComponentModel;
    let error = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
    assert!(alloc::format!("{error:?}").contains("mismatched stack contract"), "{error:?}");
}

#[test]
fn source_dyncall_preserves_debugger_frames_with_full_argument_window() {
    use alloc::sync::Arc;
    let params = (0..16).map(|i| format!("%a{i}: felt")).collect::<Vec<_>>().join(", ");
    let args = (0..16).map(|i| format!("%a{i}")).collect::<Vec<_>>().join(", ");
    let types = vec!["felt"; 16].join(", ");
    let f = parse(&format!(
        "builtin.function public extern(\"C\") @dispatch(%r0: felt,%r1: felt,%r2: felt,%r3: \
         felt,{params}) -> felt {{ %result=hir.dyncall [%r0,%r1,%r2,%r3]({args}) : \
         extern(\"component-model\") ({types}) -> felt; builtin.ret %result : (felt); }};"
    ));
    let selected = selected_procedure(&f, &SelectEnv::default());
    let manager = Arc::new(miden_assembly::DefaultSourceManager::default());
    let code = format!(
        "proc callee nop end {selected} begin {} procref.callee exec.selected push.11 assert_eq \
         end",
        crate::plan::tests::lowering_units::push_top(&(11..27).collect::<Vec<_>>()),
    );
    let package = miden_assembly::Assembler::new(manager.clone())
        .assemble_program("source_dyncall_debug_frames", code)
        .unwrap();
    crate::plan::tests::lowering_units::assert_debugger_call_frames(
        package.into(),
        manager,
        [],
        "selected",
    )
    .unwrap();
}

#[test]
fn source_convention_table_call_and_procedure_verify() {
    let text = include_str!("fixtures/indirect-table.hir").replace("\"C\"", "\"sigma\"");
    let f = parse_module_function(&text);
    let info = linked(&f);
    let env = SelectEnv {
        link_info: Some(&info),
        ..Default::default()
    };
    let selected = checked(&f, &env);
    assert_eq!(selected.abi.convention(), crate::plan::CallingConvention::SameContext);
    let (plan, frame, body) = ready(&f, &env);
    verify::verify(&plan, &frame, body).unwrap();
}

#[test]
fn procref_and_procedure_root_select_the_same_digest() {
    for operation in [
        "%root = hir.procref @callee; %r0,%r1,%r2,%r3 = hir.unpack_word %root;",
        "%r0,%r1,%r2,%r3 = hir.procedure_root @callee;",
    ] {
        let f = parse_module_function(&alloc::format!(
            r#"builtin.module public @m {{
            builtin.function private extern("C") @callee() {{ builtin.ret; }};
            builtin.function public extern("C") @dispatch() -> (felt, felt, felt, felt) {{
                {operation} builtin.ret %r0,%r1,%r2,%r3 : (felt, felt, felt, felt);
            }};
        }};"#
        ));
        let selected = checked(&f, &SelectEnv::default());
        let root = &selected.segments[0].units[0];
        assert_eq!(root.outputs.len(), 4);
        assert_eq!(
            selected.values[&root.outputs[0].value].layout.lanes(),
            if operation.contains("procref") { 4 } else { 1 }
        );
        let procedure =
            selected_procedure(&f, &SelectEnv::default()).replace("::m::callee", "callee");
        execute_code(&alloc::format!(
            "proc callee nop end {procedure} begin exec.selected procref.callee assert_eqw end"
        ))
        .unwrap();
    }
}

fn pointer_dispatch(context: bool, width: usize) -> Parsed {
    let arguments = (0..width).map(|i| alloc::format!(", %a{i}: felt")).collect::<String>();
    let uses = (0..width).map(|i| alloc::format!("%a{i}")).collect::<Vec<_>>().join(",");
    let types = vec!["felt"; width].join(",");
    let operation = if context { "dyncall_local" } else { "dynexec" };
    let convention = if context { "component-model" } else { "C" };
    parse(&alloc::format!(
        r#"builtin.function public extern("C") @dispatch(%p: ptr<[felt; 4], element>{arguments}) -> (felt, ptr<[felt; 4], element>) {{
        %r = hir.{operation} %p({uses}) : extern("{convention}") ({types}) -> (felt);
        builtin.ret %r, %p : (felt, ptr<[felt; 4], element>);
    }};"#
    ))
}

#[test]
fn pointer_dispatch_stores_one_selector_before_sixteen_arguments_and_keeps_it_live() {
    for context in [false, true] {
        let f = pointer_dispatch(context, 16);
        let selected = checked(&f, &SelectEnv::default());
        let dispatch = &selected.segments[0].units[0];
        assert_eq!(dispatch.stack_inputs.len(), 16);
        assert_eq!(dispatch.stored_inputs.len(), 1);
        assert_eq!(dispatch.effects.memory, SourceMemoryEffects::READ);
        assert!(dispatch.effects.may_trap);
        assert!(dispatch.scratch.is_empty());
        assert!(
            crate::allocate::analysis::analyze(&selected).after[&dispatch.id].contains(ValueId(0))
        );
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        assert_eq!(plan.lowering_bindings[&dispatch.id].stored_inputs.len(), 1);
        let procedure = verify::verify(&plan, &frame, body)
            .unwrap()
            .into_masm(crate::masm::ProcedureName::new("dispatch").unwrap())
            .unwrap()
            .to_pretty_string();
        let suffix = (901..925).collect::<Vec<_>>();
        let checksum = (1..=16).fold(0, |checksum, argument| 2 * checksum + argument);
        let mut expected = vec![checksum, 400];
        expected.extend(&suffix);
        execute_code(&alloc::format!(
            "proc callee {} end {procedure} begin procref.callee mem_storew_le.400 dropw {} {} \
             push.400 exec.dispatch {} end",
            vec!["push.2 mul add"; 15].join(" "),
            crate::plan::tests::lowering_units::push_top(&suffix),
            crate::plan::tests::lowering_units::push_top(&(1..=16).collect::<Vec<_>>()),
            crate::plan::tests::lowering_units::assert_top(&expected),
        ))
        .unwrap();
    }
}

#[test]
fn pointer_dispatch_context_padding_cleanup_and_invocation_mismatch() {
    let f = pointer_dispatch(true, 1);
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    let procedure = verify::verify(&plan, &frame, body.clone())
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("dispatch").unwrap())
        .unwrap()
        .to_pretty_string();
    execute_code(&alloc::format!(
        "proc callee push.1 add end {procedure} begin procref.callee mem_storew_le.400 dropw \
         push.903 push.902 push.901 push.41 push.400 exec.dispatch push.42 assert_eq push.400 \
         assert_eq push.901 assert_eq push.902 assert_eq push.903 assert_eq end"
    ))
    .unwrap();
    let mut wrong = body;
    for op in wrong.ops_mut() {
        if let AnnotatedOp::Instruction { instruction, .. } = op
            && *instruction == crate::masm::Instruction::DynCall
        {
            *instruction = crate::masm::Instruction::DynExec;
        }
    }
    assert!(verify::verify(&plan, &frame, wrong).is_err());
}

#[test]
fn pointer_dispatch_source_reselection_and_physical_mutations_are_rejected() {
    let f = pointer_dispatch(false, 1);
    let function = f.borrow();
    let env = SelectEnv::default();
    let selected = select_function(&function, &env).unwrap();
    for mutation in 0..4 {
        let mut bad = selected.clone();
        let unit = &mut bad.segments[0].units[0];
        match mutation {
            0 => unit.stored_inputs[0].value = StoredValue::Atom(Atom::new(ValueId(1), 0)),
            1 => unit.stored_inputs.clear(),
            2 => unit.effects.memory = SourceMemoryEffects::default(),
            _ => unit.outputs.clear(),
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
    drop(function);
    let (plan, frame, body) = ready(&f, &env);
    let unit = &plan.selected.segments[0].units[0];
    let slot = plan.lowering_bindings[&unit.id].stored_inputs[&StoredInputId(0)];
    let offset = frame.resolve(FrameRef::Spill(slot)).unwrap();
    let mut wrong_slot = body.clone();
    let load = wrong_slot.ops_mut().iter_mut().find(|op| matches!(op,
        AnnotatedOp::Instruction { instruction: I::LocLoad(index), unit: id } if *id == unit.id && *index == offset
    )).unwrap();
    if let AnnotatedOp::Instruction { instruction, .. } = load {
        *instruction = I::LocLoad((offset + 1).into());
    }
    assert!(verify::verify(&plan, &frame, wrong_slot).is_err());
    let mut early = body;
    let end = early
        .ops_mut()
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitEnd(id)) if *id == unit.id))
        .unwrap();
    let end = early.ops_mut().remove(end);
    let invocation = early
        .ops_mut()
        .iter()
        .position(|op| {
            matches!(
                op,
                AnnotatedOp::Instruction {
                    instruction: I::DynExec,
                    ..
                }
            )
        })
        .unwrap();
    early.ops_mut().insert(invocation, end);
    assert!(verify::verify(&plan, &frame, early).is_err());
}

#[test]
fn procref_uses_procedure_root_visibility_alias_and_intent_rules() {
    for operation in ["procref", "procedure_root"] {
        let result = if operation == "procref" {
            "%root"
        } else {
            "%r0,%r1,%r2,%r3"
        };
        for (visibility, accessible) in [("private", false), ("internal", true), ("public", true)] {
            let f = parse_module_function(&alloc::format!(
                r#"builtin.module public @m {{
                builtin.module public @nested {{
                    builtin.function {visibility} extern("C") @callee() {{ builtin.ret; }};
                }};
                builtin.function public extern("C") @dispatch() {{ {result} = hir.{operation} @nested::@callee; builtin.ret; }};
            }};"#
            ));
            assert_eq!(select_function(&f.borrow(), &SelectEnv::default()).is_ok(), accessible);
        }
        let f = parse_module_function(&alloc::format!(
            r#"builtin.module public @m {{
            builtin.function internal extern("C") @callee() {{ builtin.ret; }};
            builtin.function_alias internal @first -> @callee;
            builtin.function_alias public @second -> @first;
            builtin.function public extern("C") @dispatch() {{ {result} = hir.{operation} @second; builtin.ret; }};
        }};"#
        ));
        let procedure = selected_procedure(&f, &SelectEnv::default());
        assert!(procedure.contains("::m::second"), "{procedure}");
        let mut reference = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .iter()
            .next()
            .unwrap()
            .as_operation_ref();
        reference.borrow_mut().set_attribute(
            crate::hir::ProcedureRoot::NOTE_SCRIPT_ROOT_ATTR,
            f.context.create_attribute::<builtin::attributes::UnitAttr, _>(()),
        );
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
}

#[test]
fn procref_word_stored_in_source_local_dispatches_through_its_pointer() {
    let mut f = parse_module_function(
        r#"builtin.module public @m {
        builtin.function private extern("C") @callee(%x: felt) -> felt { builtin.ret %x : (felt); };
        builtin.function public extern("C") @dispatch(%x: felt) -> felt {
            %root = hir.procref @callee;
            %p = hir.local_address <{ local = #builtin.local_variable<0, [felt; 4]> }>;
            %r0,%r1,%r2,%r3 = hir.unpack_word %root;
            %p0 = hir.gep %p <{ indices = #builtin.array_u32<0> }>;
            %p1 = hir.gep %p <{ indices = #builtin.array_u32<1> }>;
            %p2 = hir.gep %p <{ indices = #builtin.array_u32<2> }>;
            %p3 = hir.gep %p <{ indices = #builtin.array_u32<3> }>;
            hir.store %p0, %r0 : (ptr<felt, element>, felt);
            hir.store %p1, %r1 : (ptr<felt, element>, felt);
            hir.store %p2, %r2 : (ptr<felt, element>, felt);
            hir.store %p3, %r3 : (ptr<felt, element>, felt);
            %r = hir.dynexec %p(%x) : extern("C") (felt) -> (felt);
            builtin.ret %r : (felt);
        };
    };"#,
    );
    f.function.borrow_mut().alloc_local(crate::hir::Procref::word_type());
    let (plan, frame, body) = ready(&f, &SelectEnv::default());
    assert_eq!(frame.source().extent, 4);
    let procedure = verify::verify(&plan, &frame, body)
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("dispatch").unwrap())
        .unwrap()
        .to_pretty_string()
        .replace("::m::callee", "callee");
    execute_code(&alloc::format!(
        "proc callee push.1 add end {procedure} begin push.901 push.41 exec.dispatch push.42 \
         assert_eq push.901 assert_eq end"
    ))
    .unwrap();
}
