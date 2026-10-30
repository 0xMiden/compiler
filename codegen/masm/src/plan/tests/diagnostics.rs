use alloc::{format, sync::Arc, vec};

use midenc_session::diagnostics::{
    DefaultSourceManager, DiagnosticsHandler, NullEmitter, PrintDiagnostic, SourceLanguage,
    SourceManager, Uri,
};

use super::*;

#[test]
fn report_renders_source_and_inline_ancestry() {
    let sources = Arc::new(DefaultSourceManager::default());
    let primary = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("selected.hir"),
        "selected_operation\ncaller_operation\n".into(),
    );
    let caller = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("caller.hir"),
        "outer_caller\n".into(),
    );
    let diagnostics =
        DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
    let mut error = PlanError::new(DiagnosticCategory::IllegalStackAccess, "invalid stack depth");
    error.origin = SourceLocation {
        span: crate::masm::SourceSpan::new(primary.id(), 0..18),
        inline_chain: vec![
            crate::masm::SourceSpan::new(primary.id(), 19..35),
            crate::masm::SourceSpan::new(caller.id(), 0..12),
        ],
    };
    error.step = Some(SelectedStepId(7));
    error.depth = Some(16);
    let rendered = format!("{}", PrintDiagnostic::new(error.into_report(&diagnostics)));
    for expected in [
        "IllegalStackAccess",
        "step=s7",
        "depth=16",
        "selected.hir",
        "selected_operation",
        "caller_operation",
        "caller.hir",
        "outer_caller",
        "inlined from",
    ] {
        assert!(rendered.contains(expected), "missing {expected:?}: {rendered}");
    }
}

#[test]
fn display_keeps_context_without_raw_source_spans() {
    let mut error = PlanError::new(DiagnosticCategory::UnavailableValue, "missing lane");
    error.step = Some(SelectedStepId(4));
    error.depth = Some(2);
    error.expected = Some(Atom::new(ValueId(0), 0));
    error.actual = Some(Atom::new(ValueId(1), 0));
    let message = error.to_string();
    for expected in ["UnavailableValue", "step=s4", "depth=2", "expected=", "actual="] {
        assert!(message.contains(expected), "{message}");
    }
    assert!(!message.contains("SourceSpan"), "{message}");
}

#[test]
fn report_without_resolvable_source_has_useful_fallback() {
    let sources = Arc::new(DefaultSourceManager::default());
    let source =
        sources.load(SourceLanguage::Other("test"), Uri::new("short.hir"), "short\n".into());
    let diagnostics =
        DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
    for span in [
        crate::masm::SourceSpan::UNKNOWN,
        crate::masm::SourceSpan::new(crate::masm::SourceSpan::UNKNOWN.source_id(), 10..20),
        crate::masm::SourceSpan::new(source.id(), 10..20),
    ] {
        let mut error =
            PlanError::new(DiagnosticCategory::InvalidSelectedLowering, "missing source evidence");
        error.origin.span = span;
        let rendered = format!("{}", PrintDiagnostic::new(error.into_report(&diagnostics)));
        assert!(rendered.contains("missing source evidence"), "{rendered}");
        assert!(rendered.contains("source location unavailable"), "{rendered}");
    }
}

#[test]
fn unavailable_primary_does_not_promote_inline_caller_to_failure_origin() {
    let sources = Arc::new(DefaultSourceManager::default());
    let caller = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("caller.hir"),
        "outer_caller\n".into(),
    );
    let diagnostics =
        DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
    let mut error = PlanError::new(DiagnosticCategory::UnavailableValue, "missing lane");
    error.origin.inline_chain.push(crate::masm::SourceSpan::new(caller.id(), 0..12));
    let rendered = format!("{}", PrintDiagnostic::new(error.into_report(&diagnostics)));
    assert!(rendered.contains("UnavailableValue: missing lane"), "{rendered}");
    assert!(rendered.contains("source location unavailable"), "{rendered}");
    assert!(!rendered.contains("caller.hir"), "{rendered}");
}

#[test]
fn unavailable_inline_frame_keeps_usable_frames_and_their_ordinals() {
    let sources = Arc::new(DefaultSourceManager::default());
    let primary = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("selected.hir"),
        "selected_operation\n".into(),
    );
    let caller = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("caller.hir"),
        "outer_caller\n".into(),
    );
    let diagnostics =
        DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
    let mut error = PlanError::new(DiagnosticCategory::UnavailableValue, "missing lane");
    error.origin = SourceLocation {
        span: crate::masm::SourceSpan::new(primary.id(), 0..18),
        inline_chain: vec![
            crate::masm::SourceSpan::UNKNOWN,
            crate::masm::SourceSpan::new(caller.id(), 0..12),
        ],
    };
    let rendered = format!("{}", PrintDiagnostic::new(error.into_report(&diagnostics)));
    for expected in [
        "selected.hir",
        "selected_operation",
        "caller.hir",
        "outer_caller",
        "inlined from (frame 2)",
        "one or more inline source locations unavailable",
    ] {
        assert!(rendered.contains(expected), "missing {expected:?}: {rendered}");
    }
}

#[test]
fn allocation_metrics_count_only_glue_and_use_expanded_costs() {
    let slot = SpillSlotId::canonical(Atom::new(ValueId(0), 0));
    let actions = [
        PlanAction::Swap { depth: 1 },
        PlanAction::StoreLane(slot),
        PlanAction::RunLowering {
            step: SelectedStepId(0),
        },
        PlanAction::DebugLocation(DebugLocationId(0)),
        PlanAction::LoadLane(slot),
        PlanAction::SwapWord { word: 1 },
    ];
    let metrics = ActionMetrics::of(&actions);
    assert_eq!(
        metrics,
        ActionMetrics {
            actions: 4,
            cost: 13,
            loads: 1,
            stores: 1
        }
    );
    assert_eq!(metrics.to_string(), "actions=4 cost=13 loads=1 stores=1");
    assert_eq!(
        ActionMetrics::of(&[]),
        ActionMetrics {
            actions: 0,
            cost: 0,
            loads: 0,
            stores: 0
        }
    );
}

#[test]
fn trace_delimiters_include_generated_identity_and_parameters() {
    let (plan, frame, _) = super::pipeline::ready_root(&super::pipeline::root_source(5, 2));
    let rendered = format!(
        "{}",
        PhysicalPlanTrace {
            plan: &plan,
            frame: &frame
        }
    );
    let ProcedureOrigin::Generated {
        definition,
        parameters,
    } = &plan.selected.origin
    else {
        panic!("generated fixture");
    };
    let identity = format!("generated:{}:{parameters:?}", definition.0);
    let encoded = identity
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<alloc::string::String>();
    assert_eq!(
        rendered.lines().next().unwrap(),
        format!("physical-plan identity-utf8={encoded}")
    );
    assert_eq!(rendered.lines().last().unwrap(), "physical-plan-end");
    assert_eq!(rendered.matches("physical-plan-end").count(), 1);
}

#[test]
fn generated_terminal_tail_is_reported_without_preparation_metrics() {
    use crate::verify::{
        self, DefinitionEvidence, GeneratedDefinition, SourceContext, UnitEvidence,
    };
    struct TerminalTail;
    impl GeneratedDefinition for TerminalTail {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(8004)
        }

        fn instantiate(
            &self,
            parameters: &[miden_core::Felt],
        ) -> Result<DefinitionEvidence, PlanError> {
            assert!(parameters.is_empty());
            let mut program = (*verify::tests::empty_program()).clone();
            program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            let mut terminal = SelectedLowering::fixture(LoweringKind(0), SourceStepId(0));
            terminal.effects.normal_return = false;
            terminal.effects.may_trap = true;
            program.segments[0].units =
                vec![terminal, SelectedLowering::fixture(LoweringKind(0), SourceStepId(1))];
            program.edges[0].kind = EdgeKind::Trap;
            program.source_map = (0..2)
                .map(|i| {
                    (
                        SourceStepId(i),
                        SourceRecord {
                            origin: SourceOrigin::Generated {
                                definition: self.id(),
                                stage: i,
                            },
                            location: SourceLocation::default(),
                        },
                    )
                })
                .collect();
            Ok(DefinitionEvidence {
                source_locals: SourceLocalLayout::default(),
                program,
                units: (0..2).map(|i| (SelectedStepId(i), UnitEvidence::default())).collect(),
            })
        }
    }
    let source = TerminalTail;
    let selected = verify::check_selection(
        source.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&source),
    )
    .unwrap();
    let plan = crate::allocate::allocate(selected, &Default::default()).unwrap();
    assert!(plan.decisions.preparations.contains_key(&SelectedStepId(0)));
    assert!(!plan.decisions.preparations.contains_key(&SelectedStepId(1)));
    let rendered = format!("{plan}");
    assert!(rendered.contains("unplanned-unit s1 kind=0 source=1"), "{rendered}");
    assert_eq!(rendered.matches("tier=").count(), 1, "{rendered}");
}

/// The printed plan shows a literal publication and names a folded count as a lane-free
/// dependency of its shift, since neither appears in any input list.
#[test]
fn printed_plan_shows_literal_publications_and_lane_free_inputs() {
    let source = crate::select::tests::parse(
        "builtin.function public extern(\"C\") @f(%a:u32) -> u32 { %c = arith.constant 3 : u32; \
         %r = arith.shl %a, %c; builtin.ret %r : (u32); };",
    );
    let (plan, ..) = crate::select::tests::ready(&source, &crate::select::SelectEnv::default());
    let units = &plan.selected.segments[0].units;
    assert_eq!(units[0].publication, OutputPublication::Literal);
    let count = units[0].outputs[0].value;
    let rendered = format!("{plan}");
    assert_eq!(
        rendered
            .lines()
            .filter(|line| line.trim() == "publication=Literal semantic=[]")
            .count(),
        1,
        "{rendered}"
    );
    let shift = format!("publication=Stack semantic={:?}", vec![&count]);
    assert_eq!(rendered.lines().filter(|line| line.trim() == shift).count(), 1, "{rendered}");
}
