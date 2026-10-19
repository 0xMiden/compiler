use crate::{
    allocate::{allocate, schedule::AllocationOptions, slots::assign_frame},
    lower::{expand::*, units::LoweringRegistry},
    opt::peephole,
    plan::*,
    verify,
};

#[test]
fn empty_executable_blocks_get_verified_nop_anchors() {
    let mut plan = allocate(verify::tests::empty_program(), &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let mut body =
        expand_procedure(&plan, &frame, &LoweringRegistry::default(), &ExpansionEnv::default())
            .unwrap();
    peephole::simplify_annotated(&mut body);
    let verified = verify::verify(&plan, &frame, body).unwrap();
    let procedure = verified.into_masm(crate::masm::ProcedureName::new("empty").unwrap()).unwrap();
    use midenc_hir::formatter::PrettyPrint;
    let source = procedure.to_pretty_string();
    assert_eq!(source.matches("nop").count(), 1, "{source}");
    miden_assembly::Assembler::default()
        .assemble_program("empty_pipeline", alloc::format!("{source} begin exec.empty end"))
        .unwrap();
}

use alloc::vec::Vec;

use midenc_hir::{Report, formatter::PrettyPrint};

use super::lowering_units::{assert_top, host, session, symbol};
use crate::{
    select::generated::GeneratedRoot,
    verify::{GeneratedDefinition, SourceContext, check_selection},
};

pub(crate) fn root_source(arguments: u32, results: u32) -> GeneratedRoot {
    GeneratedRoot {
        initializer: Some(symbol("initialize")),
        harness: Some(symbol("harness")),
        entry: symbol("entry"),
        entry_abi: ProcedureAbi::new(CallingConvention::SameContext, arguments, results).unwrap(),
    }
}
pub(crate) fn ready_root(
    source: &GeneratedRoot,
) -> (crate::plan::actions::ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    let selected = check_selection(
        source.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(source),
    )
    .unwrap();
    let mut plan = allocate(selected, &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let definitions = source.definitions();
    let mut registry = LoweringRegistry::default();
    for definition in &definitions {
        registry.register(definition).unwrap();
    }
    let mut body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
    peephole::simplify_annotated(&mut body);
    (plan, frame, body)
}
#[test]
fn generated_root_checks_value_identity_arithmetic_before_construction() {
    assert_eq!(
        root_source(0, u32::MAX).instantiate(&[]).unwrap_err().category,
        DiagnosticCategory::InvalidSelectedLowering,
    );
}

#[test]
fn generated_root_preserves_original_window_through_required_stages() -> Result<(), Report> {
    let core = midenc_session::LinkLibrary::core().load(&session().options)?;
    for (arguments, results) in [
        (16, 16),
        (0, 0),
        (0, 16),
        (16, 0),
        (5, 2),
        (2, 5),
        (0, 20),
        (16, 20),
        (0, 257),
        (16, 257),
    ] {
        let source = root_source(arguments, results);
        let (plan, frame, body) = ready_root(&source);
        assert_eq!(plan.selected.segments[0].units.len(), 4);
        let procedure = verify::verify(&plan, &frame, body)
            .unwrap()
            .into_masm(crate::masm::ProcedureName::new("root").unwrap())
            .unwrap()
            .to_pretty_string();
        let checks = (0..arguments)
            .map(|i| alloc::format!("dup.{i} push.{} assert_eq ", i + 1))
            .collect::<String>();
        let push = (0..results)
            .rev()
            .map(|i| alloc::format!("push.{} ", 700 + i))
            .collect::<String>();
        let mut expected = (0..results)
            .map(|i| 700 + i as usize)
            .chain((arguments..16).map(|i| i as usize + 1))
            .take(16)
            .collect::<Vec<_>>();
        expected.resize(16, 0);
        let drops = if arguments == 0 {
            String::new()
        } else {
            alloc::format!("repeat.{arguments} drop end")
        };
        let code = alloc::format!(
            "proc initialize mem_load.100 assertz push.123 mem_store.100 end proc harness \
             mem_load.100 push.123 assert_eq push.456 mem_store.101 end proc entry mem_load.100 \
             push.123 assert_eq mem_load.101 push.456 assert_eq {checks} {drops} {push} end \
             {procedure} begin exec.root sdepth push.16 assert_eq {} end",
            assert_top(&expected)
        );
        let mut assembler = miden_assembly::Assembler::default();
        assembler.link_package(core.clone(), miden_assembly::Linkage::Dynamic)?;
        let program = assembler.assemble_program("root_pipeline", code)?.try_into_program()?;
        let mut host = host().with_library(core.clone()).map_err(|e| Report::msg(e.to_string()))?;
        let inputs = (1..=16).map(miden_core::Felt::new_unchecked).collect::<Vec<_>>();
        miden_processor::FastProcessor::new(miden_processor::StackInputs::new(&inputs).unwrap())
            .execute_sync(&program, &mut host)
            .map_err(|e| Report::msg(e.to_string()))?;
    }
    Ok(())
}

#[test]
fn omitted_nop_anchor_cannot_become_an_assembled_body() {
    let plan = allocate(verify::tests::empty_program(), &AllocationOptions::default()).unwrap();
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    let body =
        expand_procedure(&plan, &frame, &LoweringRegistry::default(), &ExpansionEnv::default())
            .unwrap();
    // Low-level graph fixtures may establish an abstract empty physical proof. Such a body
    // cannot be serialized: assembly must not manufacture its missing executable Nop.
    let verified = verify::verify(&plan, &frame, body).unwrap();
    assert!(verified.into_masm(crate::masm::ProcedureName::new("empty").unwrap()).is_err());
}

use alloc::{boxed::Box, collections::BTreeMap};

use super::lowering_units::{atom, push_top};
use crate::{
    lower::units::{LoweringDefinition, Scalar, calls::ContextCall, memory::SpanningCell},
    verify::{DefinitionEvidence, VerificationError},
};

pub(crate) struct Structured;
impl GeneratedDefinition for Structured {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(8001)
    }

    fn instantiate(
        &self,
        parameters: &[miden_core::Felt],
    ) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() {
            return Err(PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "structured fixture takes no parameters",
            ));
        }
        let mut proofs = BTreeMap::new();
        let mut scalar = |definition: Scalar, id: u32, inputs: &[u32], output: u32| {
            let (unit, proof) = definition
                .select(
                    SelectedStepId(id),
                    SourceStepId(id),
                    &inputs.iter().copied().map(atom).collect::<Vec<_>>(),
                    &[atom(output)],
                )
                .unwrap();
            proofs.insert(unit.id, proof);
            unit
        };
        let outer_condition = scalar(Scalar::Neq, 0, &[10, 2], 20);
        let inner_initial = scalar(Scalar::Zero, 1, &[], 30);
        let inner_condition = scalar(Scalar::Neq, 2, &[11, 3], 21);
        let inner_next = scalar(Scalar::Add, 3, &[11, 1], 31);
        let outer_next = scalar(Scalar::Add, 4, &[10, 1], 32);
        let entry_alias = scalar(Scalar::Identity, 5, &[100], 1500);
        let true_alias = scalar(Scalar::Identity, 6, &[100], 3000);
        let false_alias = scalar(Scalar::Identity, 7, &[100], 3001);
        let final_alias = scalar(Scalar::Identity, 12, &[500], 5000);
        let scratch = ScratchRequirement {
            id: ScratchId(0),
            size: 1,
            alignment: 1,
            contiguous: true,
            address_space: AddressSpace::ProcedureLocal,
            lifetime: ScratchLifetime::Spanning,
            uses: vec![SelectedStepId(8), SelectedStepId(9)],
        };
        let (write, proof) = SpanningCell::Write.select(
            SelectedStepId(8),
            SourceStepId(8),
            &scratch,
            ContentId(0),
            &[atom(100)],
            &[],
        )?;
        proofs.insert(write.id, proof);
        let (read, proof) = SpanningCell::Read.select(
            SelectedStepId(9),
            SourceStepId(9),
            &scratch,
            ContentId(0),
            &[],
            &[atom(2000)],
        )?;
        proofs.insert(read.id, proof);
        let (call, proof) =
            join_call().select(SelectedStepId(10), SourceStepId(10), &[], None, &[])?;
        proofs.insert(call.id, proof);
        let segment = |id, parameters: Vec<u32>, units| SelectedSegment {
            id: SegmentId(id),
            parameters: parameters.into_iter().map(ValueId).collect(),
            content_parameters: vec![],
            units,
        };
        let segments = vec![
            segment(0, (0..5).chain(100..120).collect(), vec![write, entry_alias]),
            segment(1, vec![10], vec![outer_condition]),
            segment(2, vec![], vec![inner_initial]),
            segment(3, vec![11], vec![inner_condition]),
            segment(4, vec![], vec![inner_next]),
            segment(5, vec![41], vec![outer_next]),
            segment(6, vec![40], vec![read]),
            segment(7, vec![], vec![true_alias]),
            segment(8, vec![], vec![call, false_alias]),
            segment(9, core::iter::once(500).chain(600..620).collect(), vec![final_alias]),
        ];
        let edge =
            |id, source, destination: Option<u32>, kind, bindings: Vec<(u32, u32)>| TransferEdge {
                id: EdgeId(id),
                source: SegmentId(source),
                destination: destination.map(SegmentId),
                kind,
                bindings: bindings
                    .into_iter()
                    .map(|(d, s)| ParameterBinding {
                        destination: ValueId(d),
                        source: ValueId(s),
                    })
                    .collect(),
                content_bindings: vec![],
            };
        let mut edges = vec![
            edge(0, 0, Some(1), EdgeKind::Forward, vec![(10, 0)]),
            edge(1, 1, Some(2), EdgeKind::True, vec![]),
            edge(2, 2, Some(3), EdgeKind::Forward, vec![(11, 30)]),
            edge(3, 3, Some(4), EdgeKind::True, vec![]),
            edge(4, 3, Some(5), EdgeKind::False, vec![(41, 11)]),
            edge(5, 4, Some(3), EdgeKind::Backedge, vec![(11, 31)]),
            edge(6, 1, Some(6), EdgeKind::False, vec![(40, 10)]),
            edge(7, 5, Some(1), EdgeKind::Backedge, vec![(10, 32)]),
            edge(8, 6, Some(7), EdgeKind::True, vec![]),
            edge(9, 6, Some(8), EdgeKind::False, vec![]),
            edge(
                10,
                7,
                Some(9),
                EdgeKind::Forward,
                core::iter::once((500, 3000))
                    .chain((0..20).map(|i| (600 + i, 100 + i)))
                    .collect(),
            ),
            edge(
                11,
                8,
                Some(9),
                EdgeKind::Forward,
                core::iter::once((500, 3001))
                    .chain((0..20).map(|i| (600 + i, 100 + i)))
                    .collect(),
            ),
        ];
        let returns = core::iter::once(40)
            .chain([5000])
            .chain(600..620)
            .chain([2000, 1500])
            .map(|i| (i, i))
            .collect::<Vec<_>>();
        let results = returns.len() as u32;
        edges.push(edge(12, 9, None, EdgeKind::Return, returns));
        let s = |i| ControlTree::Segment(SegmentId(i));
        let e = |i| ControlTree::Edge(EdgeId(i));
        let seq = |v| ControlTree::Sequence(v);
        let inner = ControlTree::Loop {
            id: ControlId(2),
            condition: ControlId(3),
            header: SegmentId(3),

            before: Box::new(ControlTree::Segment(SegmentId(3))),
            predicate: atom(21),
            on_true: EdgeId(3),
            on_false: EdgeId(4),
            body: Box::new(seq(vec![s(4), e(5)])),
            backedges: vec![EdgeId(5)],
        };
        let outer = ControlTree::Loop {
            id: ControlId(0),
            condition: ControlId(1),
            header: SegmentId(1),

            before: Box::new(ControlTree::Segment(SegmentId(1))),
            predicate: atom(20),
            on_true: EdgeId(1),
            on_false: EdgeId(6),
            body: Box::new(seq(vec![s(2), e(2), inner, s(5), e(7)])),
            backedges: vec![EdgeId(7)],
        };
        let branch = ControlTree::If {
            id: ControlId(4),
            predicate: atom(4),
            on_true: EdgeId(8),
            on_false: EdgeId(9),
            then_body: Box::new(seq(vec![s(7), e(10)])),
            else_body: Box::new(seq(vec![s(8), e(11)])),
        };
        let mut values = BTreeMap::new();
        let mut source_map = BTreeMap::new();
        for segment in &segments {
            for (index, id) in segment.parameters.iter().enumerate() {
                values.insert(
                    *id,
                    SelectedValue {
                        id: *id,
                        origin: ValueOrigin::Parameter {
                            segment: segment.id,
                            index: index as u32,
                        },
                        layout: ValueLayout::new(midenc_hir::Type::Felt)?,
                        reconstruction: None,
                    },
                );
            }
            for unit in &segment.units {
                source_map.insert(
                    unit.source,
                    SourceRecord {
                        origin: SourceOrigin::Generated {
                            definition: self.id(),
                            stage: unit.source.0,
                        },
                        location: Default::default(),
                    },
                );
                for (index, atom) in unit.outputs.iter().enumerate() {
                    values.insert(
                        atom.value,
                        SelectedValue {
                            id: atom.value,
                            origin: ValueOrigin::Temporary {
                                step: unit.source,
                                index: index as u32,
                            },
                            layout: ValueLayout::new(midenc_hir::Type::Felt)?,
                            reconstruction: if unit.kind == Scalar::Zero.kind() {
                                Some(Reconstruction::Literal(vec![miden_core::Felt::ZERO]))
                            } else {
                                None
                            },
                        },
                    );
                }
            }
        }
        Ok(DefinitionEvidence {
            source_locals: Default::default(),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments,
                edges,
                structure: seq(vec![s(0), e(0), outer, s(6), branch, s(9), e(12)]),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 25, results)?,
                source_map,
            },
            units: proofs,
        })
    }
}
fn join_call() -> ContextCall {
    ContextCall {
        kind: LoweringKind(1002),
        abi: ProcedureAbi::new(CallingConvention::Context, 0, 0).unwrap(),
        instruction: crate::masm::Instruction::Call(symbol("callee")),
        root: ScratchId(1),
    }
}
pub(crate) fn structured_ready(
    options: &AllocationOptions,
) -> (crate::plan::actions::ProcedurePlan, FinalFrameLayout, AnnotatedMasmBody) {
    let selected = check_selection(
        Structured.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&Structured),
    )
    .unwrap();
    let mut plan = allocate(selected, options).unwrap();
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    resolve_frame_bindings(&mut plan, &frame).unwrap();
    let definitions: [Box<dyn LoweringDefinition>; 7] = [
        Box::new(Scalar::Add),
        Box::new(Scalar::Neq),
        Box::new(Scalar::Identity),
        Box::new(Scalar::Zero),
        Box::new(SpanningCell::Write),
        Box::new(SpanningCell::Read),
        Box::new(join_call()),
    ];
    let mut registry = LoweringRegistry::default();
    for definition in &definitions {
        registry.register(definition.as_ref()).unwrap();
    }
    let mut body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
    peephole::simplify_annotated(&mut body);
    (plan, frame, body)
}
#[test]
fn nested_loops_and_wide_joins_execute_all_scheduling_tiers() -> Result<(), Report> {
    use crate::allocate::schedule::SchedulingTier;
    for tier in [
        SchedulingTier::Stack,
        SchedulingTier::Loads,
        SchedulingTier::Selective,
        SchedulingTier::Normalization,
    ] {
        let options = AllocationOptions {
            force_tier: Some(tier),
            refuse_solver: true,
            ..Default::default()
        };
        let (plan, frame, body) = structured_ready(&options);
        assert_eq!(plan.selected.segments[9].parameters.len(), 21);
        assert!(plan.contracts.values().any(|contract| !contract.required_scratch.is_empty()));
        let procedure = verify::verify(&plan, &frame, body)
            .unwrap()
            .into_masm(crate::masm::ProcedureName::new("fixture").unwrap())
            .unwrap()
            .to_pretty_string();
        for (outer, inner, selector) in [(2, 3, 0), (2, 3, 1), (0, 3, 0), (2, 0, 1)] {
            let inputs =
                [0, 1, outer, inner, selector].into_iter().chain(500..520).collect::<Vec<_>>();
            let expected = [outer, 500]
                .into_iter()
                .chain(500..520)
                .chain([500, 500, 901, 903])
                .collect::<Vec<_>>();
            let code = alloc::format!(
                "proc callee repeat.16 push.999 swap.1 drop end end {procedure} begin \
                 push.903.901 {} exec.fixture {} end",
                push_top(&inputs),
                assert_top(&expected)
            );
            let program = miden_assembly::Assembler::default()
                .assemble_program("structured_pipeline", code)?
                .try_into_program()?;
            miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
                .execute_sync(&program, &mut host())
                .map_err(|e| Report::msg(alloc::format!("{tier:?}: {e}")))?;
        }
    }
    Ok(())
}

#[test]
fn peepholes_preserve_units_edges_results_and_control() {
    use super::lowering_units::{Case, Source};
    use crate::plan::actions::PlanAction;
    let source = Source {
        case: Case::Fpi(16),
        live: vec![],
    };
    let (mut plan, frame, _) = super::lowering_units::ready(&source, &AllocationOptions::default());
    plan.segment_actions
        .get_mut(&SegmentId(0))
        .unwrap()
        .splice(0..0, [PlanAction::Dup { depth: 0 }, PlanAction::Drop]);
    let definition = source.definition();
    let mut registry = LoweringRegistry::default();
    registry.register(definition.as_ref()).unwrap();
    let mut body = expand_procedure(&plan, &frame, &registry, &ExpansionEnv::default()).unwrap();
    let index = body
        .ops()
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitStart(_))))
        .unwrap()
        + 1;
    body.ops_mut().splice(
        index..index,
        [
            AnnotatedOp::Instruction {
                instruction: crate::masm::Instruction::Swap1,
                unit: SelectedStepId(0),
            },
            AnnotatedOp::Instruction {
                instruction: crate::masm::Instruction::Swap1,
                unit: SelectedStepId(0),
            },
        ],
    );
    let mut a = AnnotatedMasmBody::new();
    let mut b = AnnotatedMasmBody::new();
    for arm in [&mut a, &mut b] {
        for _ in 0..2 {
            arm.push_instruction(crate::masm::Instruction::Swap1, SelectedStepId(0));
        }
    }
    body.ops_mut().splice(
        index..index,
        [
            AnnotatedOp::Instruction {
                instruction: Primitive::Push(miden_core::Felt::ONE).instruction().unwrap(),
                unit: SelectedStepId(0),
            },
            AnnotatedOp::If {
                id: ControlId(99),
                then_body: a,
                else_body: b,
            },
        ],
    );
    peephole::simplify_annotated(&mut body);
    let Some(AnnotatedOp::If {
        then_body,
        else_body,
        ..
    }) = body.ops().iter().find(|op| {
        matches!(
            op,
            AnnotatedOp::If {
                id: ControlId(99),
                ..
            }
        )
    })
    else {
        panic!()
    };
    for arm in [then_body, else_body] {
        assert!(matches!(arm.ops(), [AnnotatedOp::Anchor(Anchor::Nop)]));
    }
    assert!(matches!(
        body.ops()[1],
        AnnotatedOp::Instruction {
            instruction: crate::masm::Instruction::Dup0,
            ..
        }
    ));
    assert!(matches!(
        body.ops()[2],
        AnnotatedOp::Instruction {
            instruction: crate::masm::Instruction::Drop,
            ..
        }
    ));
    assert!(
        !body
            .top_level_instructions()
            .any(|instruction| *instruction == crate::masm::Instruction::Swap1)
    );
    verify::verify(&plan, &frame, body.clone())
        .unwrap()
        .into_masm(crate::masm::ProcedureName::new("fixture").unwrap())
        .unwrap();
    let end = body
        .ops()
        .iter()
        .position(|op| matches!(op, AnnotatedOp::Boundary(Boundary::UnitEnd(_))))
        .unwrap();
    body.ops_mut().insert(
        end,
        AnnotatedOp::Instruction {
            instruction: crate::masm::Instruction::Swap1,
            unit: SelectedStepId(0),
        },
    );
    body.ops_mut().insert(
        end + 2,
        AnnotatedOp::Instruction {
            instruction: crate::masm::Instruction::Swap1,
            unit: SelectedStepId(u32::MAX),
        },
    );
    peephole::simplify_annotated(&mut body);
    assert_eq!(
        body.top_level_instructions()
            .filter(|instruction| **instruction == crate::masm::Instruction::Swap1)
            .count(),
        2
    );
    assert!(verify::verify(&plan, &frame, body).is_err());
}

#[test]
fn debug_decorators_keep_anchors_through_optimization_and_assembly() -> Result<(), Report> {
    use crate::plan::actions::PlanAction;
    let mut plan = allocate(verify::tests::empty_program(), &AllocationOptions::default()).unwrap();
    plan.segment_actions
        .get_mut(&SegmentId(0))
        .unwrap()
        .push(PlanAction::DebugLocation(DebugLocationId(7)));
    let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
    let env = ExpansionEnv {
        debug: BTreeMap::from([(DebugLocationId(7), DebugDecorator::InlineCallClear)]),
        ..Default::default()
    };
    let mut body = expand_procedure(&plan, &frame, &LoweringRegistry::default(), &env)?;
    peephole::simplify_annotated(&mut body);
    let anchor = body
        .ops()
        .iter()
        .position(|op| {
            matches!(
                op,
                AnnotatedOp::Anchor(Anchor::Debug {
                    id: DebugLocationId(7),
                    ..
                })
            )
        })
        .unwrap();
    assert!(matches!(
        body.ops()[anchor - 1],
        AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0)))
    ));
    let procedure =
        verify::verify(&plan, &frame, body)?.into_masm(crate::masm::ProcedureName::main())?;
    assert_eq!(procedure.body().iter().filter(|op|matches!(op,crate::masm::Op::Inst(inst)if matches!(inst.inner(),crate::masm::Instruction::DebugInlineCallClear))).count(),1);
    assert_eq!(procedure.body().iter().filter(|op|matches!(op,crate::masm::Op::Inst(inst)if matches!(inst.inner(),crate::masm::Instruction::Nop))).count(),1);
    let manager = alloc::sync::Arc::new(miden_assembly::DefaultSourceManager::default());
    let mut module = crate::masm::Module::new_executable();
    use midenc_session::diagnostics::IntoDiagnostic;
    module.define_procedure(procedure, manager.clone()).into_diagnostic()?;
    let program = miden_assembly::Assembler::new(manager)
        .assemble_program("debug_anchor", module)?
        .try_into_program()?;
    miden_processor::FastProcessor::new(miden_processor::StackInputs::default())
        .execute_sync(&program, &mut host())
        .map_err(|e| Report::msg(e.to_string()))?;
    Ok(())
}

#[test]
fn real_memmove_observes_each_requested_preparation_tier() -> Result<(), Report> {
    use super::lowering_units::{Case, Source, execute_memmove};
    use crate::{
        allocate::{
            analysis::analyze,
            schedule::{SchedulingTier, prepare_unit},
            state::{AllocationContext, AllocationState},
        },
        plan::actions::PlanAction,
    };
    for tier in [
        SchedulingTier::Stack,
        SchedulingTier::Loads,
        SchedulingTier::Selective,
        SchedulingTier::Normalization,
    ] {
        let source = Source {
            case: Case::Memmove,
            live: vec![],
        };
        let (mut plan, ..) = super::lowering_units::ready(&source, &AllocationOptions::default());
        let analysis = analyze(&plan.selected);
        let mut context = AllocationContext::new(&plan.selected, &analysis);
        context.options = AllocationOptions {
            force_tier: Some(tier),
            refuse_solver: true,
            ..Default::default()
        };
        let mut contract = plan.contracts[&SegmentId(0)].clone();
        let mut prelude = Vec::new();
        if tier == SchedulingTier::Loads {
            let first = contract.stack_top_first.remove(0);
            assert_eq!(first, atom(0));
            let slot = SpillSlotId::canonical(first);
            contract.required_spill_slots.push((slot, first));
            prelude.push(PlanAction::StoreLane(slot));
        }
        let state = AllocationState::at_entry(&contract, SegmentId(0), &context).unwrap();
        let preparation = prepare_unit(
            &state,
            &plan.selected.segments[0].units[0],
            &Default::default(),
            &context,
        )
        .unwrap();
        assert_eq!(preparation.tier, tier);
        prelude.extend(preparation.actions);
        prelude.push(PlanAction::RunLowering {
            step: SelectedStepId(0),
        });
        plan.spill_slots = preparation.slot_requests.into_iter().collect();
        plan.segment_actions.insert(SegmentId(0), prelude);
        let frame = assign_frame(&plan, &SourceLocalLayout::default()).unwrap();
        resolve_frame_bindings(&mut plan, &frame).unwrap();
        let definition = source.definition();
        let mut registry = LoweringRegistry::default();
        registry.register(definition.as_ref()).unwrap();
        let verified = finalize(&plan, &frame, &registry, &ExpansionEnv::default())?;
        let procedure = verified.into_masm(crate::masm::ProcedureName::new("fixture").unwrap())?;
        let mut expected = (0..48).map(|i| (i * 37 + 11) as u8).collect::<Vec<_>>();
        expected.copy_within(1..20, 3);
        execute_memmove(&procedure, 1025, 1027, 19, Some(&expected), false)?;
    }
    Ok(())
}
