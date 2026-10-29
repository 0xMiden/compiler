use super::*;

#[test]
fn registered_hir_families_have_positive_and_negative_source_coverage() {
    use alloc::collections::BTreeSet;

    use midenc_hir::dialects::{builtin, debuginfo};

    use crate::{cf, ub};
    let context = Context::default();
    let dialects = [
        context.get_or_register_dialect::<builtin::BuiltinDialect>(),
        context.get_or_register_dialect::<midenc_dialect_arith::ArithDialect>(),
        context.get_or_register_dialect::<cf::ControlFlowDialect>(),
        context.get_or_register_dialect::<midenc_dialect_scf::ScfDialect>(),
        context.get_or_register_dialect::<ub::UndefinedBehaviorDialect>(),
        context.get_or_register_dialect::<midenc_dialect_hir::HirDialect>(),
        context.get_or_register_dialect::<debuginfo::DebugInfoDialect>(),
        context.get_or_register_dialect::<crate::wasm::WasmDialect>(),
    ];
    let registered = dialects
        .iter()
        .flat_map(|d| d.registered_ops())
        .filter(|op| op.implements::<dyn crate::HirSelection>())
        .map(|op| op.to_string())
        .collect::<BTreeSet<_>>();
    let mut covered = BTreeSet::new();
    let mut fixtures = [
        include_str!("fixtures/nested-before.hir"),
        include_str!("fixtures/aliases.hir"),
        include_str!("fixtures/terminal-control.hir"),
        include_str!("fixtures/switch-cases.hir"),
        include_str!("fixtures/default-switch.hir"),
        include_str!("fixtures/trapping-before.hir"),
        include_str!("fixtures/indirect-before.hir"),
        include_str!("fixtures/transparent-casts.hir"),
    ]
    .into_iter()
    .map(|text| (parse(text), false))
    .collect::<Vec<_>>();
    fixtures.push((parse_module_function(include_str!("fixtures/indirect-table.hir")), true));
    fixtures.push((debug_fixture(), false));
    fixtures.push((
        parse_module_function(
            r#"builtin.module public @references {
        builtin.function public extern("C") @callee() { builtin.ret; };
        builtin.function public extern("C") @dispatch() -> (felt, felt, felt, felt) {
            %r0, %r1, %r2, %r3 = hir.procedure_root @callee;
            builtin.ret %r0, %r1, %r2, %r3 : (felt, felt, felt, felt);
        };
    };"#,
        ),
        false,
    ));
    fixtures.push((
        parse_module_function(
            r#"builtin.module public @pointer_references {
        builtin.function public extern("C") @callee(%x: felt) -> felt {
            builtin.ret %x : (felt);
        };
        builtin.function public extern("C") @dispatch(%p: ptr<[felt; 4], element>, %x: felt) -> (felt, [felt; 4]) {
            %root = hir.procref @callee;
            %same = hir.dynexec %p(%x) : extern("C") (felt) -> (felt);
            %fresh = hir.dyncall_local %p(%same) : extern("component-model") (felt) -> (felt);
            builtin.ret %fresh, %root : (felt, [felt; 4]);
        };
    };"#,
        ),
        false,
    ));
    let outputs = (0..16).map(|i| alloc::format!("%o{i}")).collect::<Vec<_>>().join(", ");
    fixtures.push((
        parse(&alloc::format!(
            "builtin.function public extern(\"C\") @foreign(%s: felt, %p: felt, %r0: felt, %r1: \
             felt, %r2: felt, %r3: felt) -> felt {{ {outputs} = hir.exec_fpi [%s, %p, %r0, %r1, \
             %r2, %r3](%r0); builtin.ret %o0 : (felt); }};"
        )),
        false,
    ));
    fixtures.push((super::inline_asm::fixture("push.1 add"), false));
    fixtures.extend(super::calls::fixtures());
    fixtures.extend(super::arithmetic::fixtures());
    fixtures.extend(super::memory::fixtures());
    fixtures.push((
        super::memory::gep_fixture(
            midenc_hir::AddressSpace::Element,
            midenc_hir::Type::U32,
            1,
            false,
        ),
        false,
    ));
    fixtures.extend(super::intrinsics::fixtures());
    fixtures.push((super::intrinsics::println_fixture(false), false));
    for immediate in [false, true] {
        fixtures.push((super::intrinsics::generic_event_fixture(immediate), false));
    }
    fixtures.extend(
        super::intrinsics::system_event_cases()
            .into_iter()
            .map(|(event, n)| (super::intrinsics::system_event_fixture(event, n), false)),
    );
    for (f, needs_link) in fixtures {
        let function = f.borrow();
        let info = needs_link.then(|| linked(&f));
        let env = SelectEnv {
            link_info: info.as_ref(),
            ..Default::default()
        };
        let selected = select_function(&function, &env).unwrap();
        for record in selected.source_map.values() {
            if let SourceOrigin::Hir(op) = record.origin {
                let op = op.borrow();
                if op.implements::<dyn crate::HirSelection>() {
                    covered.insert(op.name().to_string());
                }
            }
        }
        check_selection(
            selected.clone(),
            &SourceContext::Hir {
                function: &function,
                env: &env,
            },
        )
        .unwrap();
        for id in selected.source_map.keys() {
            let mut bad = selected.clone();
            bad.source_map.remove(id);
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
        for (segment_index, segment) in selected.segments.iter().enumerate() {
            for unit_index in 0..segment.units.len() {
                let mut bad = selected.clone();
                bad.segments[segment_index].units.remove(unit_index);
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
        for i in 0..selected.edges.len() {
            let mut bad = selected.clone();
            bad.edges.remove(i);
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
    assert_eq!(registered.len(), 133);
    assert_eq!(covered, registered);
}

#[test]
fn unsupported_final_operations_and_width_changing_aliases_fail_explicitly() {
    for text in [
        "builtin.function public extern(\"C\") @unsupported(%a: i32,%b: i32) -> i32 { \
         %result=arith.sdiv %a,%b; builtin.ret %result : (i32); };",
        "builtin.function public extern(\"C\") @unsupported(%a: u32) -> u32 { \
         %result=builtin.unrealized_conversion_cast %a <{ ty=#builtin.type<u32> }>; builtin.ret \
         %result : (u32); };",
        "builtin.function public extern(\"C\") @unsupported(%a: ptr<u32, byte>) -> u64 { \
         %result=hir.ptr_to_int %a <{ ty=#builtin.type<u64> }>; builtin.ret %result : (u64); };",
    ] {
        let f = parse_unverified(text);
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
}

#[test]
fn real_source_bindings_effects_and_alias_maps_cannot_be_changed_by_candidates() {
    let f = parse(include_str!("fixtures/nested-before.hir"));
    let function = f.borrow();
    let env = SelectEnv::default();
    let selected = select_function(&function, &env).unwrap();
    for mutation in 0..4 {
        let mut bad = selected.clone();
        match mutation {
            0 => bad.edges[0].bindings.reverse(),
            1 => {
                bad.edges.iter_mut().find(|e| e.kind == EdgeKind::True).unwrap().kind =
                    EdgeKind::False
            }
            2 => {
                bad.segments.iter_mut().find(|s| !s.units.is_empty()).unwrap().units[0]
                    .effects
                    .memory = SourceMemoryEffects::WRITE
            }
            _ => bad.abi = ProcedureAbi::new(CallingConvention::SameContext, 4, 2).unwrap(),
        };
        assert!(
            check_selection(
                bad,
                &SourceContext::Hir {
                    function: &function,
                    env: &env
                }
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn source_mutation_after_candidate_selection_is_detected() {
    let f = parse(
        "builtin.function public extern(\"C\") @source(%a: felt) -> felt { %unused=arith.constant \
         7 : u32; builtin.ret %a : (felt); };",
    );
    let function = f.borrow();
    let env = SelectEnv::default();
    let candidate = select_function(&function, &env).unwrap();
    let mut constant =
        function.entry_block().borrow().body().front().get().unwrap().as_operation_ref();
    constant.borrow_mut().erase();
    assert!(
        check_selection(
            candidate,
            &SourceContext::Hir {
                function: &function,
                env: &env
            }
        )
        .is_err()
    );
}

#[test]
fn debug_descriptions_have_coverage_but_do_not_extend_semantic_liveness() {
    let f = debug_fixture();
    let selected = checked(&f, &SelectEnv::default());
    assert!(selected.segments[0].units.is_empty());
    assert!(selected.source_map.len() >= 6);
    let analysis = crate::allocate::analysis::analyze(&selected);
    let first = selected.segments[0].parameters[0];
    assert!(!analysis.live_in[&SegmentId(0)].contains(first));
    let function = f.borrow();
    let env = SelectEnv::default();
    let mut bad = select_function(&function, &env).unwrap();
    let id = *bad.source_map.keys().nth(2).unwrap();
    bad.source_map.remove(&id);
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

fn debug_fixture() -> Parsed {
    use midenc_hir::{
        Builder, OpBuilder, SourceSpan,
        dialects::debuginfo::{
            DIBuilder,
            attributes::{Expression, Variable},
        },
        interner::Symbol,
    };
    let f = parse(
        "builtin.function public extern(\"C\") @debug(%debug_only: felt,%result: felt) -> felt { \
         builtin.ret %result : (felt); };",
    );
    let entry = f.borrow().entry_block();
    let value = entry.borrow().arguments()[0] as midenc_hir::ValueRef;
    let terminator = entry.borrow().terminator().unwrap();
    let mut builder = OpBuilder::new(f.context.clone());
    builder.set_insertion_point_before(terminator);
    let variable = Variable::new(Symbol::intern("x"), Symbol::intern("test.rs"), 1, None);
    builder.debug_value(value, variable.clone(), SourceSpan::UNKNOWN).unwrap();
    builder
        .debug_declare(variable.clone(), Expression::default(), SourceSpan::UNKNOWN)
        .unwrap();
    builder.debug_kill(variable, SourceSpan::UNKNOWN).unwrap();
    f
}

use midenc_hir::{
    OpPrinter, RegionBranchOpInterface, RegionBranchPoint, RegionSuccessorIter,
    derive::{OpParser, operation},
    dialects::test::TestDialect,
};

#[derive(OpParser, midenc_hir::derive::OpPrinter)]
#[operation(dialect=TestDialect, implements(RegionBranchOpInterface,OpPrinter))]
pub struct UnregisteredRegion {}

impl RegionBranchOpInterface for UnregisteredRegion {
    fn get_successor_regions(&self, _: RegionBranchPoint) -> RegionSuccessorIter<'_> {
        RegionSuccessorIter::new(self.as_operation(), [])
    }
}

#[test]
fn a_region_interface_alone_does_not_authorize_final_selection() {
    let f = parse(
        "builtin.function public extern(\"C\") @unknown() { test.unregistered_region; \
         builtin.ret; };",
    );
    let op = f
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .front()
        .get()
        .unwrap()
        .as_operation_ref();
    assert!(op.borrow().implements::<dyn RegionBranchOpInterface>());
    let error = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
    assert!(error.to_string().contains("unsupported final HIR operation"));
}

#[test]
fn source_convention_classification_preserves_context_boundaries() {
    use crate::plan::CallingConvention::{Context, SameContext};

    for (name, expected) in [
        ("C", SameContext),
        ("fast", SameContext),
        ("wasm", SameContext),
        ("sigma", SameContext),
        ("custom", SameContext),
        ("component-model", Context),
    ] {
        let f = parse(&alloc::format!(
            "builtin.function public extern(\"{name}\") @source() {{ builtin.ret; }};"
        ));
        let selected = checked(&f, &SelectEnv::default());
        assert_eq!(selected.abi.convention(), expected, "{name}");
    }
}

#[test]
fn malformed_switch_case_region_counts_and_duplicate_labels_fail_explicitly() {
    use midenc_hir::dialects::builtin::attributes::{Array, U32ArrayAttr};
    for cases in [vec![7, 7, 9], vec![7, 8, 9, 10]] {
        let f = parse(include_str!("fixtures/switch-cases.hir"));
        let mut switch = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        let cases = f.context.create_attribute::<U32ArrayAttr, _>(Array::from_iter(cases));
        switch.borrow_mut().set_property("cases", cases.as_attribute_ref()).unwrap();
        assert!(select_function(&f.borrow(), &SelectEnv::default()).is_err());
    }
}

#[test]
fn literal_source_rules_publish_complete_distinct_raw_lanes() {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{Immediate, SourceSpan, Type, dialects::builtin::BuiltinOpBuilder};
    for (immediate, expected) in [
        (Immediate::I8(-1), vec![255]),
        (Immediate::U64(0x22334455_66778899), vec![0x66778899, 0x22334455]),
        (Immediate::I128(-1), vec![u32::MAX as usize; 4]),
        (
            Immediate::Felt(miden_core::Felt::new_unchecked(0xffff_ffff_0000_0000)),
            vec![0xffff_ffff_0000_0000usize],
        ),
    ] {
        let ty = immediate.ty();
        let mut test = midenc_hir::testing::Test::new("literal", &[], &[ty]);
        {
            let mut builder = test.function_builder();
            let value = builder.imm(immediate, SourceSpan::UNKNOWN);
            builder.ret([value], SourceSpan::UNKNOWN).unwrap();
        }
        let f = Parsed {
            context: test.context_rc(),
            function: test.function(),
        };
        execute(&f, &SelectEnv::default(), &[], &expected);
        let selected = checked(&f, &SelectEnv::default());
        assert_eq!(selected.segments[0].units[0].outputs.len(), expected.len());
    }
    let mut test = midenc_hir::testing::Test::new("unsupported_float", &[], &[Type::F64]);
    {
        let mut builder = test.function_builder();
        builder.ret_imm(Immediate::F64(1.0), SourceSpan::UNKNOWN).unwrap();
    }
    assert!(select_function(&test.function().borrow(), &SelectEnv::default()).is_err());
}

#[test]
fn constant_result_type_must_equal_its_immediate_type_even_at_the_same_raw_width() {
    use midenc_hir::{Type, Value};
    for ty in [Type::U8, Type::U16, Type::I32] {
        let f = parse(
            "builtin.function public extern(\"C\") @constant() { %unused=arith.constant 70000 : \
             u32; builtin.ret; };",
        );
        let op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        let mut result = op.borrow().results().all()[0];
        result.borrow_mut().set_type(ty.clone());
        assert_malformed_source_rejected(&f, &SelectEnv::default());
        assert_eq!(result.borrow().ty(), &ty);
    }
}

#[test]
fn function_entry_and_all_terminal_return_tuples_match_the_full_signature() {
    use midenc_hir::{SourceSpan, Type, Value};
    for text in [
        "builtin.function public extern(\"C\") @ret(%a: felt) -> u32 { builtin.ret %a : (felt); };",
        "builtin.function public extern(\"C\") @ret(%a: felt) -> (felt,felt) { builtin.ret %a : \
         (felt); };",
        "builtin.function public extern(\"C\") @ret_imm() -> u32 { builtin.ret_imm 7 : felt; };",
        "builtin.function public extern(\"C\") @ret_imm() { builtin.ret_imm 7 : felt; };",
        "builtin.function public extern(\"C\") @dispatch(%p: i1,%a: felt) -> u32 { cf.cond_br %p \
         ^first,^second : (i1); ^first: builtin.ret %a : (felt); ^second: builtin.ret_imm 7 : \
         felt; };",
    ] {
        let f = parse(text);
        assert_malformed_source_rejected(&f, &SelectEnv::default());
    }
    for arity in [false, true] {
        let f = parse("builtin.function public extern(\"C\") @entry(%a: felt) { builtin.ret; };");
        let entry = f.borrow().entry_block();
        if arity {
            f.context.append_block_argument(entry, Type::Felt, SourceSpan::UNKNOWN);
        } else {
            let mut arg = entry.borrow().arguments()[0];
            arg.borrow_mut().set_type(Type::U32);
        }
        assert_malformed_source_rejected(&f, &SelectEnv::default());
    }
}

#[test]
fn generic_event_any_felt_id_keeps_the_same_source_category() {
    use midenc_hir::Immediate;
    for id in [
        0,
        700001,
        crate::Event::PrintLn.as_event_id().as_u64(),
        miden_core::events::SystemEvent::TraceEvent.event_id().as_u64(),
        0xffff_ffff_0000_0000,
    ] {
        let f = super::intrinsics::generic_event_fixture(true);
        let mut op = f
            .borrow()
            .entry_block()
            .borrow()
            .body()
            .front()
            .get()
            .unwrap()
            .as_operation_ref();
        *op.borrow_mut()
            .downcast_mut::<midenc_dialect_hir::EmitEventImm>()
            .unwrap()
            .get_event_id_mut() = Immediate::Felt(miden_core::Felt::new_unchecked(id));
        let selected = checked(&f, &SelectEnv::default());
        let unit = &selected.segments[0].units[0];
        let evidence = crate::select::evidence(&f.borrow(), &SelectEnv::default()).unwrap();
        assert!(matches!(
            evidence.source.units[&unit.id].generated_event,
            Some(verify::GeneratedEvent::Immediate)
        ));
        assert_eq!(unit.effects.memory, SourceMemoryEffects::default());
        let (plan, frame, body) = ready(&f, &SelectEnv::default());
        verify::verify(&plan, &frame, body).unwrap();
    }
}

#[test]
fn nested_selection_failure_retains_the_actual_operation_span_and_inline_chain() {
    use midenc_hir::{
        Spanned,
        dialects::debuginfo::attributes::{
            INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr, InlineCallFrame,
        },
    };
    let text = r#"builtin.function public extern("C") @unknown(%p: i1, %a: felt) -> felt {
        %result = scf.if %p then {
            test.unregistered_region;
            scf.yield %a : (felt);
        } else {
            scf.yield %a : (felt);
        } : (i1) -> felt;
        builtin.ret %result : (felt);
    };"#;
    let f = parse(text);
    let mut unsupported = None;
    f.borrow().as_operation().prewalk_all(|operation| {
        if operation.is::<UnregisteredRegion>() {
            unsupported = Some(operation.as_operation_ref());
        }
    });
    let mut unsupported = unsupported.unwrap();
    let span = unsupported.borrow().span();
    assert!(!span.source_id().is_unknown());
    assert_eq!(span.start().to_u32() as usize, text.find("test.unregistered_region").unwrap());
    let chain = f
        .context
        .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(vec![InlineCallFrame {
            name: "inlined".into(),
            linkage_name: None,
            file: "selection.hir".into(),
            line: 1,
            column: 1,
            call_file: "selection.hir".into(),
            call_line: 1,
            call_column: 1,
        }]))
        .as_attribute_ref();
    unsupported.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, chain);
    let report = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
    let diagnostic = report.downcast_ref::<PlanError>().unwrap();
    assert_eq!(diagnostic.category, DiagnosticCategory::InvalidSelectedLowering);
    assert_eq!(diagnostic.origin.span, span);
    assert_eq!(diagnostic.origin.inline_chain.len(), 1);
    assert_eq!(diagnostic.origin.inline_chain[0].source_id(), span.source_id());
    assert_eq!(diagnostic.origin.inline_chain[0].start().to_u32(), 0);
    assert_eq!(diagnostic.step, None);
    assert_eq!(diagnostic.depth, None);
    assert_eq!(diagnostic.expected, None);
    assert_eq!(diagnostic.actual, None);
}

#[test]
fn oversized_context_signature_retains_function_source_span() {
    use midenc_hir::Spanned;
    let params = (0..17).map(|i| alloc::format!("%p{i}: felt")).collect::<Vec<_>>().join(", ");
    let f = parse(&alloc::format!(
        "builtin.function public extern(\"component-model\") @source({params}) {{ builtin.ret; }};"
    ));
    let span = f.borrow().span();
    assert!(!span.source_id().is_unknown());
    let report = select_function(&f.borrow(), &SelectEnv::default()).unwrap_err();
    let diagnostic = report.downcast_ref::<PlanError>().unwrap();
    assert_eq!(diagnostic.origin.span, span);
    assert_eq!(diagnostic.step, None);
    assert_eq!(diagnostic.depth, None);
}
