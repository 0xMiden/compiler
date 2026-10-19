use alloc::{collections::BTreeMap, vec, vec::Vec};

use super::{
    edges::unit,
    normalization::{DiscardDefinition, fixture_body, fixture_frame},
};
use crate::{
    allocate::{allocate, schedule::AllocationOptions},
    lower::expand::{AnnotatedMasmBody, AnnotatedOp, Boundary, expand_allocation_action},
    masm::Instruction,
    plan::{
        actions::{PlanAction, ProcedurePlan},
        *,
    },
    verify::*,
};

pub(super) fn body(plan: &ProcedurePlan, frame: &FinalFrameLayout) -> AnnotatedMasmBody {
    fn actions(
        out: &mut AnnotatedMasmBody,
        actions: &[PlanAction],
        plan: &ProcedurePlan,
        frame: &FinalFrameLayout,
    ) {
        for action in actions {
            if let PlanAction::RunLowering { step } = action {
                let unit = plan
                    .selected
                    .segments
                    .iter()
                    .flat_map(|s| &s.units)
                    .find(|u| u.id == *step)
                    .unwrap();
                out.push(AnnotatedOp::Boundary(Boundary::UnitStart(*step)));
                match unit.kind.0 {
                    0 => {
                        for _ in &unit.stack_inputs {
                            out.push_instruction(Instruction::Drop, *step);
                        }
                    }
                    1 => {} // selected representation alias: unchanged physical lanes
                    2 => {
                        for atom in unit.outputs.iter().rev() {
                            let Some(Reconstruction::Literal(lanes)) =
                                &plan.selected.values[&atom.value].reconstruction
                            else {
                                panic!("literal fixture")
                            };
                            out.push_instruction(
                                Primitive::Push(lanes[usize::from(atom.lane)])
                                    .instruction()
                                    .unwrap(),
                                *step,
                            );
                        }
                    }
                    3 => out.push_instruction(Instruction::Add, *step),
                    4 => out.push_instruction(Instruction::Neq, *step),
                    5 => {
                        let fact = unit.address_results[0];
                        out.push_instruction(
                            Instruction::Locaddr(
                                frame
                                    .resolve(FrameRef::Scratch {
                                        object: fact.object,
                                        offset: fact.offset,
                                    })
                                    .unwrap()
                                    .into(),
                            ),
                            *step,
                        );
                    }
                    6 => out.push_instruction(Instruction::Exec(target("callee")), *step),
                    7 => {
                        out.push_instruction(
                            Instruction::Locaddr(
                                frame
                                    .resolve(FrameRef::Scratch {
                                        object: ScratchId(0),
                                        offset: 0,
                                    })
                                    .unwrap()
                                    .into(),
                            ),
                            *step,
                        );
                        out.push_instruction(Instruction::MemStore, *step);
                    }
                    8 => {
                        out.push_instruction(
                            Instruction::Locaddr(
                                frame
                                    .resolve(FrameRef::Scratch {
                                        object: ScratchId(0),
                                        offset: 0,
                                    })
                                    .unwrap()
                                    .into(),
                            ),
                            *step,
                        );
                        out.push_instruction(Instruction::MemLoad, *step);
                    }
                    9 => {} // conservative possible-write summary, no write on this path
                    10 => out.push_instruction(Instruction::Exec(target("truncate_stack")), *step),
                    _ => panic!("fixture kind"),
                }
                out.push(AnnotatedOp::Boundary(Boundary::UnitEnd(*step)));
            } else if let Some(instruction) = expand_allocation_action(action, frame).unwrap() {
                out.push_instruction(instruction, SelectedStepId(u32::MAX));
            }
        }
    }
    fn flag(out: &mut AnnotatedMasmBody, id: ControlId, value: bool) {
        out.push(AnnotatedOp::Boundary(Boundary::Control(id)));
        out.push_instruction(
            Primitive::Push(miden_core::Felt::new_unchecked(u64::from(value)))
                .instruction()
                .unwrap(),
            SelectedStepId(u32::MAX),
        );
    }
    fn walk(
        tree: &ControlTree,
        plan: &ProcedurePlan,
        frame: &FinalFrameLayout,
        loops: &mut Vec<(ControlId, Vec<EdgeId>)>,
    ) -> AnnotatedMasmBody {
        let mut out = AnnotatedMasmBody::new();
        match tree {
            ControlTree::Segment(id) => {
                out.push(AnnotatedOp::Boundary(Boundary::Segment(*id)));
                actions(&mut out, &plan.segment_actions[id], plan, frame);
            }
            ControlTree::Edge(id) => {
                out.push(AnnotatedOp::Boundary(Boundary::Edge(*id)));
                actions(&mut out, &plan.edge_actions[id], plan, frame);
                if let Some((control, _)) = loops.iter().rev().find(|(_, edges)| edges.contains(id))
                {
                    flag(&mut out, *control, true);
                }
            }
            ControlTree::Sequence(children) => {
                for child in children {
                    for op in walk(child, plan, frame, loops).ops() {
                        out.push(op.clone());
                    }
                }
            }
            ControlTree::If {
                id,
                on_true,
                on_false,
                then_body,
                else_body,
                ..
            } => {
                let arm = |edge, tree: &ControlTree, loops: &mut Vec<_>| {
                    let mut arm = walk(&ControlTree::Edge(edge), plan, frame, loops);
                    for op in walk(tree, plan, frame, loops).ops() {
                        arm.push(op.clone());
                    }
                    arm
                };
                out.push(AnnotatedOp::If {
                    id: *id,
                    then_body: arm(*on_true, then_body, loops),
                    else_body: arm(*on_false, else_body, loops),
                });
            }
            ControlTree::Loop {
                id,
                condition,
                header,
                on_true,
                on_false,
                body,
                backedges,
                ..
            } => {
                loops.push((*id, backedges.clone()));
                let mut taken = walk(&ControlTree::Edge(*on_true), plan, frame, loops);
                for op in walk(body, plan, frame, loops).ops() {
                    taken.push(op.clone());
                }
                loops.pop();
                let mut exited = walk(&ControlTree::Edge(*on_false), plan, frame, loops);
                flag(&mut exited, *id, false);
                let mut inner = walk(&ControlTree::Segment(*header), plan, frame, loops);
                inner.push(AnnotatedOp::If {
                    id: *condition,
                    then_body: taken,
                    else_body: exited,
                });
                flag(&mut out, *id, true);
                out.push(AnnotatedOp::While {
                    id: *id,
                    body: inner,
                });
            }
        }
        out
    }
    walk(&plan.selected.structure, plan, frame, &mut vec![])
}

fn frame(plan: &ProcedurePlan) -> FinalFrameLayout {
    crate::allocate::slots::assign_frame(plan, &SourceLocalLayout::default()).unwrap()
}
pub(super) fn render(body: &AnnotatedMasmBody) -> alloc::string::String {
    body.ops()
        .iter()
        .map(|op| match op {
            AnnotatedOp::Boundary(_) => alloc::string::String::new(),
            AnnotatedOp::Instruction { instruction, .. } => alloc::format!("{instruction} "),
            AnnotatedOp::If {
                then_body,
                else_body,
                ..
            } => alloc::format!("if.true {} else {} end ", render(then_body), render(else_body)),
            AnnotatedOp::While { body, .. } => alloc::format!("while.true {} end ", render(body)),
            _ => panic!("fixture syntax"),
        })
        .collect()
}

fn execute(plan: &ProcedurePlan, arguments: &[u64], expected: &[u64]) {
    use miden_processor::{DefaultHost, FastProcessor, StackInputs};
    let frame = frame(plan);
    let ready = bound(plan, &frame);
    let body = body(&ready, &frame);
    verify(&ready, &frame, body.clone()).unwrap();
    let pushes: alloc::string::String =
        arguments.iter().rev().map(|v| alloc::format!("push.{v} ")).collect();
    let assertions: alloc::string::String = expected
        .iter()
        .copied()
        .chain(1000..1016)
        .map(|v| alloc::format!("push.{v} assert_eq "))
        .collect();
    let source = alloc::format!(
        "proc callee nop end @locals({}) proc test {} end begin {pushes} exec.test {assertions} \
         end",
        frame.aligned_size(),
        render(&body)
    );
    let program = miden_assembly::Assembler::default()
        .assemble_program("allocator_contracts", source)
        .unwrap()
        .try_into_program()
        .unwrap();
    let suffix: Vec<_> = (1000..1016).map(miden_core::Felt::new_unchecked).collect();
    FastProcessor::new(StackInputs::new(&suffix).unwrap())
        .execute_sync(&program, &mut DefaultHost::default())
        .unwrap();
}

fn target(name: &str) -> crate::masm::InvocationTarget {
    crate::masm::InvocationTarget::Symbol(crate::masm::Ident::new(name).unwrap())
}

fn bound(plan: &ProcedurePlan, frame: &FinalFrameLayout) -> ProcedurePlan {
    let mut ready = plan.clone();
    crate::lower::expand::resolve_frame_bindings(&mut ready, frame).unwrap();
    ready
}

#[test]
fn declared_address_results_cross_an_edge_but_cannot_certify_wrong_instructions() {
    let mut pointer = request(0, &[], &[100]);
    pointer.kind = LoweringKind(5);
    pointer.scratch = vec![
        ScratchRequirement {
            id: ScratchId(0),
            size: 4,
            alignment: 4,
            contiguous: true,
            address_space: AddressSpace::ProcedureLocal,
            lifetime: ScratchLifetime::Spanning,
            uses: vec![pointer.id],
        },
        ScratchRequirement {
            id: ScratchId(1),
            size: 4,
            alignment: 4,
            contiguous: true,
            address_space: AddressSpace::ProcedureLocal,
            lifetime: ScratchLifetime::Spanning,
            uses: vec![pointer.id],
        },
    ];
    pointer.address_results = vec![AddressFact {
        pointer: ValueId(100),
        object: ScratchId(0),
        offset: 1,
        accessible: 3,
    }];
    let source = source(
        vec![
            segment(0, &[], vec![pointer]),
            segment(1, &[200], vec![request(1, &[200], &[])]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[(200, 100)]),
            edge(1, 1, None, EdgeKind::Return, &[]),
        ],
        seq(vec![s(0), e(0), s(1), e(1)]),
        0,
    );
    let plan = allocate(checked(&source), &AllocationOptions::default()).unwrap();
    assert_eq!(
        plan.contracts[&SegmentId(1)].address_facts,
        vec![AddressFact {
            pointer: ValueId(200),
            object: ScratchId(0),
            offset: 1,
            accessible: 3
        }]
    );
    let frame = frame(&plan);
    let mut ready = bound(&plan, &frame);
    let correct = body(&ready, &frame);
    verify(&ready, &frame, correct.clone()).unwrap();
    // Eliminate the later contract check: rejection must come from publication itself.
    ready.contracts.get_mut(&SegmentId(1)).unwrap().address_facts.clear();
    for replacement in [
        Primitive::Push(miden_core::Felt::ONE).instruction().unwrap(),
        Instruction::Locaddr(0.into()),
        Instruction::Locaddr(2.into()),
        Instruction::Locaddr(5.into()),
    ] {
        let mut corrupt = AnnotatedMasmBody::new();
        for op in correct.ops() {
            corrupt.push(match op {
                AnnotatedOp::Instruction {
                    instruction: Instruction::Locaddr(_),
                    unit,
                } if *unit == SelectedStepId(0) => AnnotatedOp::Instruction {
                    instruction: replacement.clone(),
                    unit: *unit,
                },
                other => other.clone(),
            });
        }
        assert_eq!(
            verify(&ready, &frame, corrupt).unwrap_err().category,
            DiagnosticCategory::InvalidEdgeTransfer
        );
    }
}

#[test]
fn address_provenance_follows_checked_alias_outputs_and_duplicate_edge_bindings() {
    let mut pointer = request(0, &[], &[100]);
    pointer.kind = LoweringKind(5);
    pointer.scratch = vec![ScratchRequirement {
        id: ScratchId(0),
        size: 4,
        alignment: 4,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: ScratchLifetime::Spanning,
        uses: vec![pointer.id],
    }];
    pointer.address_results = vec![AddressFact {
        pointer: ValueId(100),
        object: ScratchId(0),
        offset: 0,
        accessible: 4,
    }];
    let mut alias = request(1, &[100], &[101]);
    alias.kind = LoweringKind(1);
    let mut fixture = source(
        vec![
            segment(0, &[], vec![pointer, alias]),
            segment(1, &[200, 201], vec![request(2, &[200, 201], &[])]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[(200, 101), (201, 101)]),
            edge(1, 1, None, EdgeKind::Return, &[]),
        ],
        seq(vec![s(0), e(0), s(1), e(1)]),
        0,
    );
    fixture.0.values.get_mut(&ValueId(101)).unwrap().reconstruction =
        Some(Reconstruction::Alias(vec![a(100)]));
    let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
    assert_eq!(plan.contracts[&SegmentId(1)].address_facts.len(), 2);
    assert!(plan.contracts[&SegmentId(1)].aliases.pairs().contains(&(a(200), a(201))));
    assert_eq!(plan.contracts[&SegmentId(1)].stack_top_first, vec![a(200)]);
    let frame = frame(&plan);
    let ready = bound(&plan, &frame);
    verify(&ready, &frame, body(&ready, &frame)).unwrap();
}

#[test]
fn agreed_empty_join_without_common_live_representation_must_repair() {
    use super::edges::state;
    use crate::allocate::{
        analysis::analyze,
        contracts::{Incoming, choose},
        state::AllocationContext,
    };
    let source = diamond_source();
    let program = checked(&source);
    let analysis = analyze(&program);
    let context = AllocationContext::new(&program, &analysis);
    let left = state(
        vec![],
        vec![
            (SpillSlotId::canonical(a(1)), a(1)),
            (SpillSlotId::canonical(a(2)), a(2)),
            (SpillSlotId::canonical(a(3)), a(3)),
        ],
    );
    let mut right = left.clone();
    let old = right.slots.remove(&SpillSlotId::canonical(a(1))).unwrap();
    right.slots.insert(SpillSlotId::edge_temporary(EdgeId(3), 0), old);
    let incoming = [
        Incoming {
            edge: &program.edges[2],
            state: &left,
        },
        Incoming {
            edge: &program.edges[3],
            state: &right,
        },
    ];
    let choice = choose(&program, &analysis, &context, SegmentId(3), &incoming, None).unwrap();
    assert!(
        choice.contract.stack_top_first.contains(&a(1))
            || choice.contract.required_spill_slots.iter().any(|(_, atom)| *atom == a(1))
    );
    assert!(choice.repairs.values().any(|repair| !repair.actions.is_empty()));
    // Only the two shared cells are cached obligations, including when resident.
    assert!(
        choice
            .contract
            .required_spill_slots
            .contains(&(SpillSlotId::canonical(a(2)), a(2)))
    );
    assert!(
        choice
            .contract
            .required_spill_slots
            .contains(&(SpillSlotId::canonical(a(3)), a(3)))
    );
}

#[test]
fn common_cached_slots_prefer_canonical_and_forget_one_arm_only_facts() {
    use super::edges::state;
    use crate::allocate::{
        analysis::analyze,
        contracts::{Incoming, choose},
        state::AllocationContext,
    };
    let source = diamond_source();
    let program = checked(&source);
    let analysis = analyze(&program);
    let context = AllocationContext::new(&program, &analysis);
    let canonical = SpillSlotId::canonical(a(1));
    let duplicate = SpillSlotId::edge_temporary(EdgeId(2), 0);
    let one_arm = SpillSlotId::edge_temporary(EdgeId(2), 1);
    let left = state(
        vec![a(1), a(2), a(3)],
        vec![(canonical, a(1)), (duplicate, a(1)), (one_arm, a(2))],
    );
    let right = state(vec![a(1), a(2), a(3)], vec![(canonical, a(1)), (duplicate, a(1))]);
    let incoming = [
        Incoming {
            edge: &program.edges[2],
            state: &left,
        },
        Incoming {
            edge: &program.edges[3],
            state: &right,
        },
    ];
    let choice = choose(&program, &analysis, &context, SegmentId(3), &incoming, None).unwrap();
    assert_eq!(choice.contract.required_spill_slots, vec![(canonical, a(1))]);
    assert!(choice.repairs.values().all(|repair| repair.actions.is_empty()));
}

#[test]
fn join_candidates_include_actual_edge_and_immediate_successor_costs() {
    use super::edges::state;
    use crate::allocate::{
        analysis::analyze,
        contracts::{Incoming, choose},
        state::AllocationContext,
    };
    let mut witnessed = false;
    for inputs in [
        vec![1, 2, 3],
        vec![1, 3, 2],
        vec![2, 1, 3],
        vec![2, 3, 1],
        vec![3, 1, 2],
        vec![3, 2, 1],
    ] {
        let base = diamond_source();
        let mut with = base.clone();
        with.0.segments[3].units = vec![request(2, &inputs, &[])];
        with.0.edges[4].bindings.clear();
        with.0.abi = ProcedureAbi::new(CallingConvention::SameContext, 4, 0).unwrap();
        with.0.source_map.insert(
            SourceStepId(2),
            SourceRecord {
                origin: SourceOrigin::Generated {
                    definition: GeneratedDefinitionId(460),
                    stage: 2,
                },
                location: SourceLocation::default(),
            },
        );
        let program = checked(&with);
        let analysis = analyze(&program);
        let context = AllocationContext::new(&program, &analysis);
        let left = state(vec![a(1), a(2), a(3)], vec![]);
        let right = state(vec![a(3), a(2), a(1)], vec![]);
        let incoming = [
            Incoming {
                edge: &program.edges[2],
                state: &left,
            },
            Incoming {
                edge: &program.edges[3],
                state: &right,
            },
        ];
        let choice = choose(&program, &analysis, &context, SegmentId(3), &incoming, None).unwrap();
        assert!(choice.costs.len() >= 2);
        let base_program = checked(&base);
        let base_analysis = analyze(&base_program);
        let base_context = AllocationContext::new(&base_program, &base_analysis);
        let baseline =
            choose(&base_program, &base_analysis, &base_context, SegmentId(3), &incoming, None)
                .unwrap();
        witnessed |= baseline.contract.stack_top_first != choice.contract.stack_top_first;
    }
    assert!(
        witnessed,
        "ignoring immediate preparation must change at least one chosen contract"
    );
}

#[derive(Clone)]
pub(super) struct Source(pub(super) SelectedProgram);
impl GeneratedDefinition for Source {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(460)
    }

    fn instantiate(&self, _: &[miden_core::Felt]) -> Result<DefinitionEvidence, VerificationError> {
        Ok(DefinitionEvidence {
            source_locals: SourceLocalLayout::default(),
            units: self
                .0
                .segments
                .iter()
                .flat_map(|s| &s.units)
                .map(|u| {
                    let mut proof = UnitEvidence::default();
                    if matches!(u.kind.0, 6 | 10) {
                        proof.calls.push(CallSite {
                            instruction: Instruction::Exec(target(if u.kind.0 == 6 {
                                "callee"
                            } else {
                                "truncate_stack"
                            })),
                            abi: ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap(),
                            inputs: vec![],
                            path: vec![],
                            truncate: u.kind.0 == 10,
                            outputs: vec![],
                            publications: vec![],
                            selector: None,
                        });
                    }
                    (u.id, proof)
                })
                .collect(),
            program: self.0.clone(),
        })
    }
}
pub(super) fn checked(source: &Source) -> CheckedSelectedProgram {
    check_selection(source.0.clone(), &SourceContext::Generated(source)).unwrap()
}
pub(super) fn edge(
    id: u32,
    source: u32,
    destination: Option<u32>,
    kind: EdgeKind,
    bindings: &[(u32, u32)],
) -> TransferEdge {
    TransferEdge {
        id: EdgeId(id),
        source: SegmentId(source),
        destination: destination.map(SegmentId),
        kind,
        bindings: bindings
            .iter()
            .map(|(d, s)| ParameterBinding {
                destination: ValueId(*d),
                source: ValueId(*s),
            })
            .collect(),
        content_bindings: vec![],
    }
}
pub(super) fn segment(
    id: u32,
    parameters: &[u32],
    units: Vec<SelectedLowering>,
) -> SelectedSegment {
    SelectedSegment {
        id: SegmentId(id),
        parameters: parameters.iter().copied().map(ValueId).collect(),
        content_parameters: vec![],
        units,
    }
}
pub(super) fn s(id: u32) -> ControlTree {
    ControlTree::Segment(SegmentId(id))
}
pub(super) fn e(id: u32) -> ControlTree {
    ControlTree::Edge(EdgeId(id))
}
pub(super) fn seq(children: Vec<ControlTree>) -> ControlTree {
    ControlTree::Sequence(children)
}
pub(super) fn a(id: u32) -> Atom {
    Atom::new(ValueId(id), 0)
}
pub(super) fn source(
    segments: Vec<SelectedSegment>,
    edges: Vec<TransferEdge>,
    structure: ControlTree,
    results: u32,
) -> Source {
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
                    layout: ValueLayout::new(midenc_hir::Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
        }
        for unit in &segment.units {
            source_map.insert(
                unit.source,
                SourceRecord {
                    origin: SourceOrigin::Generated {
                        definition: GeneratedDefinitionId(460),
                        stage: unit.source.0,
                    },
                    location: SourceLocation::default(),
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
                        layout: ValueLayout::new(midenc_hir::Type::Felt).unwrap(),
                        reconstruction: None,
                    },
                );
            }
        }
    }
    Source(SelectedProgram {
        origin: ProcedureOrigin::Generated {
            definition: GeneratedDefinitionId(460),
            parameters: vec![],
        },
        abi: ProcedureAbi::new(
            CallingConvention::SameContext,
            segments[0].parameters.len() as u32,
            results,
        )
        .unwrap(),
        values,
        segments,
        edges,
        structure,
        source_map,
    })
}
pub(super) fn request(id: u32, inputs: &[u32], outputs: &[u32]) -> SelectedLowering {
    let mut unit = unit(inputs.iter().copied().map(a).collect(), vec![]);
    unit.id = SelectedStepId(id);
    unit.source = SourceStepId(id);
    unit.outputs = outputs.iter().copied().map(a).collect();
    unit
}

fn inherited_source() -> Source {
    let params: Vec<_> = (0..20).collect();
    source(
        vec![segment(0, &params, vec![]), segment(1, &[], vec![])],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[]),
            edge(1, 1, None, EdgeKind::Return, &[(0, 0)]),
        ],
        seq(vec![s(0), e(0), s(1), e(1)]),
        1,
    )
}

#[test]
fn inherited_entries_keep_twenty_owned_lanes_without_normalizing() {
    let source = inherited_source();
    let plan = allocate(checked(&source), &AllocationOptions::default()).unwrap();
    assert_eq!(
        plan.contracts[&SegmentId(1)].stack_top_first,
        (0..20).map(a).collect::<Vec<_>>()
    );
    assert!(plan.edge_actions[&EdgeId(0)].is_empty());
    let frame = fixture_frame(&plan);
    verify(&plan, &frame, fixture_body(&plan, &frame)).unwrap();
}

pub(super) fn diamond_source() -> Source {
    let branch = ControlTree::If {
        id: ControlId(0),
        predicate: a(0),
        on_true: EdgeId(0),
        on_false: EdgeId(1),
        then_body: alloc::boxed::Box::new(seq(vec![s(1), e(2)])),
        else_body: alloc::boxed::Box::new(seq(vec![s(2), e(3)])),
    };
    source(
        vec![
            segment(0, &[0, 1, 2, 3], vec![]),
            segment(1, &[], vec![request(0, &[1, 2], &[])]),
            segment(2, &[], vec![request(1, &[2, 3], &[])]),
            segment(3, &[], vec![]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::True, &[]),
            edge(1, 0, Some(2), EdgeKind::False, &[]),
            edge(2, 1, Some(3), EdgeKind::Forward, &[]),
            edge(3, 2, Some(3), EdgeKind::Forward, &[]),
            edge(4, 3, None, EdgeKind::Return, &[(1, 1), (2, 2), (3, 3)]),
        ],
        seq(vec![s(0), branch, s(3), e(4)]),
        3,
    )
}

#[test]
fn delayed_join_is_independent_of_predecessor_storage_order() {
    let source = diamond_source();
    let mut forward_stats = crate::allocate::PlanningStats::default();
    let first = crate::allocate::allocate_inner(
        checked(&source),
        &AllocationOptions::default(),
        &mut forward_stats,
    )
    .unwrap();
    let mut reversed = source.clone();
    reversed.0.segments[1..].reverse();
    reversed.0.edges.reverse();
    let mut stats = crate::allocate::PlanningStats {
        reverse_ready: true,
        ..Default::default()
    };
    let second = crate::allocate::allocate_inner(
        checked(&reversed),
        &AllocationOptions::default(),
        &mut stats,
    )
    .unwrap();
    let visits = |stats: &crate::allocate::PlanningStats| {
        stats
            .events
            .iter()
            .filter_map(|event| match event {
                crate::allocate::PlanningEvent::Body(id) => Some(*id),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_ne!(visits(&forward_stats), visits(&stats));
    assert_eq!(first.contracts, second.contracts);
    assert_eq!(first.segment_actions, second.segment_actions);
    assert_eq!(first.edge_actions, second.edge_actions);
    assert_eq!(first.spill_slots, second.spill_slots);
    execute(&first, &[0, 101, 202, 303], &[101, 202, 303]);
    execute(&first, &[1, 101, 202, 303], &[101, 202, 303]);
}

pub(super) fn loop_source() -> Source {
    let mut compare = request(0, &[10, 2], &[20]);
    compare.kind = LoweringKind(4);
    let mut advance_a = request(1, &[10, 1], &[30]);
    advance_a.kind = LoweringKind(3);
    let mut advance_b = request(2, &[11, 3], &[31]);
    advance_b.kind = LoweringKind(3);
    let loop_tree = ControlTree::Loop {
        id: ControlId(0),
        condition: ControlId(1),
        header: SegmentId(1),
        predicate: a(20),
        on_true: EdgeId(1),
        on_false: EdgeId(2),
        body: alloc::boxed::Box::new(seq(vec![s(2), e(3)])),
        backedges: vec![EdgeId(3)],
    };
    source(
        vec![
            segment(0, &[0, 1, 2, 3], vec![]),
            segment(1, &[10, 11], vec![compare]),
            segment(2, &[], vec![advance_a, advance_b]),
            segment(3, &[40, 41], vec![]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[(10, 0), (11, 0)]),
            edge(1, 1, Some(2), EdgeKind::True, &[]),
            edge(2, 1, Some(3), EdgeKind::False, &[(40, 10), (41, 11)]),
            edge(3, 2, Some(1), EdgeKind::Backedge, &[(10, 30), (11, 31)]),
            edge(4, 3, None, EdgeKind::Return, &[(40, 40), (41, 41)]),
        ],
        seq(vec![s(0), e(0), loop_tree, s(3), e(4)]),
        2,
    )
}

#[test]
fn equal_initial_loop_parameters_diverge_and_headers_never_replan() {
    use crate::allocate::{PlanningEvent, PlanningStats, allocate_inner};
    let source = loop_source();
    let mut stats = PlanningStats::default();
    let plan = allocate_inner(checked(&source), &AllocationOptions::default(), &mut stats).unwrap();
    assert!(plan.contracts[&SegmentId(1)].aliases.pairs().is_empty());
    for id in 0..4 {
        assert_eq!(
            stats
                .events
                .iter()
                .filter(|event| **event == PlanningEvent::Body(SegmentId(id)))
                .count(),
            1
        );
    }
    let freeze = stats
        .events
        .iter()
        .position(|event| *event == PlanningEvent::Freeze(SegmentId(1)))
        .unwrap();
    let body = stats
        .events
        .iter()
        .position(|event| *event == PlanningEvent::Body(SegmentId(2)))
        .unwrap();
    let backedge = stats
        .events
        .iter()
        .position(|event| *event == PlanningEvent::Backedge(EdgeId(3)))
        .unwrap();
    assert!(freeze < body && body < backedge);
    execute(&plan, &[0, 1, 3, 2], &[3, 6]);
    execute(&plan, &[5, 1, 5, 2], &[5, 5]);
    let mut reversed = source.clone();
    reversed.0.segments[1..].reverse();
    reversed.0.edges.reverse();
    let mut reverse_stats = PlanningStats {
        reverse_ready: true,
        ..Default::default()
    };
    let other =
        allocate_inner(checked(&reversed), &AllocationOptions::default(), &mut reverse_stats)
            .unwrap();
    assert_eq!(plan.contracts, other.contracts);
    assert_eq!(plan.edge_actions, other.edge_actions);
}

fn wide_loop_source(identity: bool) -> Source {
    let mut fixture = loop_source();
    for id in 100..120 {
        fixture.0.segments[0].parameters.push(ValueId(id));
        fixture.0.values.insert(
            ValueId(id),
            SelectedValue {
                id: ValueId(id),
                origin: ValueOrigin::Parameter {
                    segment: SegmentId(0),
                    index: id - 96,
                },
                layout: ValueLayout::new(midenc_hir::Type::Felt).unwrap(),
                reconstruction: None,
            },
        );
        fixture.0.edges.last_mut().unwrap().bindings.push(ParameterBinding {
            destination: ValueId(id),
            source: ValueId(id),
        });
    }
    let mut cache = request(8, &[], &[]);
    cache.stored_inputs = vec![StoredInput {
        id: StoredInputId(0),
        value: StoredValue::Atom(a(2)),
    }];
    fixture.0.source_map.insert(
        cache.source,
        SourceRecord {
            origin: SourceOrigin::Generated {
                definition: GeneratedDefinitionId(460),
                stage: 8,
            },
            location: SourceLocation::default(),
        },
    );
    fixture.0.segments[0].units.push(cache);
    let consume = request(9, &(100..116).collect::<Vec<_>>(), &[]);
    fixture.0.source_map.insert(
        consume.source,
        SourceRecord {
            origin: SourceOrigin::Generated {
                definition: GeneratedDefinitionId(460),
                stage: 9,
            },
            location: SourceLocation::default(),
        },
    );
    fixture.0.segments[2].units.insert(0, consume);
    if identity {
        fixture.0.segments[2].units.pop();
        fixture.0.values.remove(&ValueId(31));
        fixture.0.source_map.remove(&SourceStepId(2));
        fixture.0.edges[3].bindings[1].source = ValueId(11);
    }
    fixture.0.abi = ProcedureAbi::new(CallingConvention::SameContext, 24, 22).unwrap();
    fixture
}

#[test]
fn invariant_slots_and_resident_caches_survive_while_carried_cells_change() {
    for identity in [false, true] {
        let fixture = wide_loop_source(identity);
        let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
        let header = &plan.contracts[&SegmentId(1)];
        assert!(header.required_spill_slots.contains(&(SpillSlotId::canonical(a(119)), a(119))));
        assert!(header.required_spill_slots.contains(&(SpillSlotId::canonical(a(2)), a(2))));
        assert!(
            header.stack_top_first.contains(&a(2)),
            "resident cache must remain resident as well"
        );
        assert!(header.required_spill_slots.contains(&(SpillSlotId::canonical(a(11)), a(11))));
        let backedge = &plan.edge_actions[&EdgeId(3)];
        assert!(!backedge.contains(&PlanAction::StoreLane(SpillSlotId::canonical(a(119)))));
        assert!(!backedge.contains(&PlanAction::StoreLane(SpillSlotId::canonical(a(2)))));
        assert_eq!(
            backedge.contains(&PlanAction::StoreLane(SpillSlotId::canonical(a(11)))),
            !identity
        );
        let mut arguments = vec![0, 1, 2, 3];
        arguments.extend(104..124);
        let mut expected = vec![2, if identity { 0 } else { 6 }];
        expected.extend(104..124);
        execute(&plan, &arguments, &expected);
    }
}

#[test]
fn lower_id_changing_parameter_cannot_hide_a_resident_invariant_cache() {
    let mut cache = request(8, &[], &[]);
    cache.stored_inputs = vec![StoredInput {
        id: StoredInputId(0),
        value: StoredValue::Atom(a(100)),
    }];
    let mut compare = request(0, &[10, 2], &[20]);
    compare.kind = LoweringKind(4);
    let mut next = request(1, &[10, 1], &[30]);
    next.kind = LoweringKind(3);
    let tree = ControlTree::Loop {
        id: ControlId(0),
        condition: ControlId(1),
        header: SegmentId(1),
        predicate: a(20),
        on_true: EdgeId(1),
        on_false: EdgeId(2),
        body: alloc::boxed::Box::new(seq(vec![s(2), e(3)])),
        backedges: vec![EdgeId(3)],
    };
    let fixture = source(
        vec![
            segment(0, &[100, 1, 2], vec![cache]),
            segment(1, &[10], vec![compare]),
            segment(2, &[], vec![next]),
            segment(3, &[40], vec![]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[(10, 100)]),
            edge(1, 1, Some(2), EdgeKind::True, &[]),
            edge(2, 1, Some(3), EdgeKind::False, &[(40, 10)]),
            edge(3, 2, Some(1), EdgeKind::Backedge, &[(10, 30)]),
            edge(4, 3, None, EdgeKind::Return, &[(40, 40), (100, 100)]),
        ],
        seq(vec![s(0), e(0), tree, s(3), e(4)]),
        2,
    );
    let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
    let header = &plan.contracts[&SegmentId(1)];
    let slot = SpillSlotId::canonical(a(100));
    assert!(header.stack_top_first.contains(&a(100)));
    assert!(
        header.required_spill_slots.contains(&(slot, a(100))),
        "the lower-ID changing parameter must not consume the invariant cache's identity"
    );
    assert!(header.aliases.pairs().is_empty());
    // A parameter's canonical namespace cannot be written before that definition's entry.
    let preheader_stores: Vec<_> = plan.segment_actions[&SegmentId(0)]
        .iter()
        .filter_map(|action| match action {
            PlanAction::StoreLane(slot) => Some(*slot),
            _ => None,
        })
        .collect();
    assert_eq!(preheader_stores, vec![slot]);
    let analysis = crate::allocate::analysis::analyze(&plan.selected);
    let context = crate::allocate::state::AllocationContext::new(&plan.selected, &analysis);
    let entry =
        crate::allocate::state::AllocationState::from_contract(&plan.contracts[&SegmentId(0)])
            .unwrap();
    let cache = &plan.selected.segments[0].units[0];
    let live_after = analysis.after[&cache.id].iter().collect();
    let preparation =
        crate::allocate::schedule::prepare_unit(&entry, cache, &live_after, &context).unwrap();
    assert_eq!(preparation.slot_requests, alloc::collections::BTreeSet::from([slot]));
    assert!(!preparation.state.slots.contains_key(&SpillSlotId::canonical(a(10))));
    assert!(!plan.edge_actions[&EdgeId(3)].contains(&PlanAction::StoreLane(slot)));
    execute(&plan, &[0, 1, 3], &[3, 0]);
    execute(&plan, &[3, 1, 3], &[3, 3]);
}

#[test]
fn carried_old_pointer_cannot_publish_a_different_current_address() {
    for (object, offset, identical) in
        [(ScratchId(1), 1, false), (ScratchId(0), 2, false), (ScratchId(0), 0, true)]
    {
        let objects: Vec<_> = (0..2)
            .map(|id| ScratchRequirement {
                id: ScratchId(id),
                size: 4,
                alignment: 4,
                contiguous: true,
                address_space: AddressSpace::ProcedureLocal,
                lifetime: ScratchLifetime::Spanning,
                uses: vec![SelectedStepId(0), SelectedStepId(1)],
            })
            .collect();
        let mut old = request(0, &[], &[100]);
        old.kind = LoweringKind(5);
        old.scratch = objects.clone();
        old.address_results = vec![AddressFact {
            pointer: ValueId(100),
            object: ScratchId(0),
            offset: 0,
            accessible: 1,
        }];
        let mut current = request(1, &[], &[101]);
        current.kind = LoweringKind(5);
        current.scratch = objects;
        current.address_results = vec![AddressFact {
            pointer: ValueId(101),
            object,
            offset,
            accessible: 1,
        }];
        let fixture = source(
            vec![segment(0, &[], vec![old, current, request(2, &[101, 100], &[])])],
            vec![edge(0, 0, None, EdgeKind::Return, &[])],
            seq(vec![s(0), e(0)]),
            0,
        );
        let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
        let frame = frame(&plan);
        let ready = bound(&plan, &frame);
        let original = body(&ready, &frame);
        verify(&ready, &frame, original.clone()).unwrap();
        let mut mutation = AnnotatedMasmBody::new();
        let mut replaced = 0;
        for op in original.ops() {
            mutation.push(match op {
                AnnotatedOp::Instruction {
                    instruction: Instruction::Locaddr(_),
                    unit,
                } if *unit == SelectedStepId(1) => {
                    replaced += 1;
                    AnnotatedOp::Instruction {
                        instruction: Instruction::Dup0,
                        unit: *unit,
                    }
                }
                other => other.clone(),
            });
        }
        assert_eq!(replaced, 1);
        let result = verify(&ready, &frame, mutation);
        if identical {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().category, DiagnosticCategory::InvalidEdgeTransfer);
        }
    }
}

#[test]
fn completion_replaces_old_output_facts_before_publishing_current_address() {
    use super::edges::state;
    let stale = SpillSlotId::canonical(a(100));
    let retained = SpillSlotId::canonical(a(1));
    let mut state = state(vec![a(0)], vec![(stale, a(100)), (retained, a(1))]);
    state.aliases = LaneAliases::new(vec![(a(100), a(200)), (a(1), a(2))]);
    state.addresses = vec![
        AddressFact {
            pointer: ValueId(100),
            object: ScratchId(0),
            offset: 0,
            accessible: 4,
        },
        AddressFact {
            pointer: ValueId(200),
            object: ScratchId(0),
            offset: 2,
            accessible: 1,
        },
    ];
    state.scratch = vec![
        ScratchCellFact {
            object: ScratchId(0),
            offset: 0,
            content: ContentRequirement::Atom(a(100)),
        },
        ScratchCellFact {
            object: ScratchId(0),
            offset: 1,
            content: ContentRequirement::Atom(a(1)),
        },
    ];
    let mut unit = request(0, &[0], &[100]);
    unit.scratch = vec![ScratchRequirement {
        id: ScratchId(1),
        size: 4,
        alignment: 4,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: ScratchLifetime::Spanning,
        uses: vec![unit.id],
    }];
    let current = AddressFact {
        pointer: ValueId(100),
        object: ScratchId(1),
        offset: 1,
        accessible: 2,
    };
    unit.address_results = vec![current];
    state.complete(&unit).unwrap();
    assert_eq!(state.stack_atoms().collect::<Vec<_>>(), vec![a(100)]);
    assert!(!state.slots.contains_key(&stale));
    assert_eq!(state.slots[&retained], StoredValue::Atom(a(1)));
    assert!(
        state.slot_requests.contains(&stale),
        "old storage requests remain part of the physical frame domain"
    );
    assert_eq!(state.aliases.pairs(), &[(a(1), a(2))]);
    assert_eq!(
        state
            .addresses
            .iter()
            .filter(|fact| fact.pointer == ValueId(100))
            .copied()
            .collect::<Vec<_>>(),
        vec![current]
    );
    assert!(state.addresses.iter().any(|fact| fact.pointer == ValueId(200)));
    assert!(state.scratch.contains(&ScratchCellFact {
        object: ScratchId(0),
        offset: 0,
        content: ContentRequirement::Initialized
    }));
    assert!(
        !state
            .scratch
            .iter()
            .any(|fact| fact.content == ContentRequirement::Atom(a(100)))
    );
    assert!(state.scratch.contains(&ScratchCellFact {
        object: ScratchId(0),
        offset: 1,
        content: ContentRequirement::Atom(a(1))
    }));
    assert!(state.available_values.contains(&ValueId(100)));
}

fn nested_source() -> Source {
    let mut outer_condition = request(0, &[10, 2], &[20]);
    outer_condition.kind = LoweringKind(4);
    let mut inner_initial = request(1, &[], &[30]);
    inner_initial.kind = LoweringKind(2);
    let mut inner_condition = request(2, &[11, 3], &[21]);
    inner_condition.kind = LoweringKind(4);
    let mut inner_next = request(3, &[11, 1], &[31]);
    inner_next.kind = LoweringKind(3);
    let mut outer_next = request(4, &[10, 1], &[32]);
    outer_next.kind = LoweringKind(3);
    let inner = ControlTree::Loop {
        id: ControlId(2),
        condition: ControlId(3),
        header: SegmentId(3),
        predicate: a(21),
        on_true: EdgeId(3),
        on_false: EdgeId(4),
        body: alloc::boxed::Box::new(seq(vec![s(4), e(5)])),
        backedges: vec![EdgeId(5)],
    };
    let outer = ControlTree::Loop {
        id: ControlId(0),
        condition: ControlId(1),
        header: SegmentId(1),
        predicate: a(20),
        on_true: EdgeId(1),
        on_false: EdgeId(6),
        body: alloc::boxed::Box::new(seq(vec![s(2), e(2), inner, s(5), e(7)])),
        backedges: vec![EdgeId(7)],
    };
    let exits = ControlTree::If {
        id: ControlId(4),
        predicate: a(4),
        on_true: EdgeId(8),
        on_false: EdgeId(9),
        then_body: alloc::boxed::Box::new(seq(vec![s(7), e(10)])),
        else_body: alloc::boxed::Box::new(seq(vec![s(8), e(11)])),
    };
    let mut fixture = source(
        vec![
            segment(0, &[0, 1, 2, 3, 4], vec![]),
            segment(1, &[10], vec![outer_condition]),
            segment(2, &[], vec![inner_initial]),
            segment(3, &[11], vec![inner_condition]),
            segment(4, &[], vec![inner_next]),
            segment(5, &[41], vec![outer_next]),
            segment(6, &[40], vec![]),
            segment(7, &[], vec![]),
            segment(8, &[], vec![]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[(10, 0)]),
            edge(1, 1, Some(2), EdgeKind::True, &[]),
            edge(2, 2, Some(3), EdgeKind::Forward, &[(11, 30)]),
            edge(3, 3, Some(4), EdgeKind::True, &[]),
            edge(4, 3, Some(5), EdgeKind::False, &[(41, 11)]),
            edge(5, 4, Some(3), EdgeKind::Backedge, &[(11, 31)]),
            edge(6, 1, Some(6), EdgeKind::False, &[(40, 10)]),
            edge(7, 5, Some(1), EdgeKind::Backedge, &[(10, 32)]),
            edge(8, 6, Some(7), EdgeKind::True, &[]),
            edge(9, 6, Some(8), EdgeKind::False, &[]),
            edge(10, 7, None, EdgeKind::Return, &[(40, 40), (1, 1)]),
            edge(11, 8, None, EdgeKind::Return, &[(40, 40), (3, 3)]),
        ],
        seq(vec![s(0), e(0), outer, s(6), exits]),
        2,
    );
    fixture.0.values.get_mut(&ValueId(30)).unwrap().reconstruction =
        Some(Reconstruction::Literal(vec![miden_core::Felt::ZERO]));
    fixture
}

#[test]
fn nested_zero_trip_loops_establish_distinct_downstream_exit_states_once() {
    use crate::allocate::{PlanningEvent, PlanningStats, allocate_inner};
    let fixture = nested_source();
    let mut stats = PlanningStats::default();
    let plan =
        allocate_inner(checked(&fixture), &AllocationOptions::default(), &mut stats).unwrap();
    for id in 0..9 {
        assert_eq!(
            stats
                .events
                .iter()
                .filter(|event| **event == PlanningEvent::Body(SegmentId(id)))
                .count(),
            1
        );
    }
    for (selector, expected) in [(0, vec![2, 3]), (1, vec![2, 1])] {
        execute(&plan, &[0, 1, 2, 3, selector], &expected);
    }
    execute(&plan, &[0, 1, 0, 3, 0], &[0, 3]);
    execute(&plan, &[0, 1, 2, 0, 1], &[2, 1]);
}

#[test]
fn phased_same_context_call_inherits_wide_state_and_live_cached_slots() {
    let mut cache = request(0, &[], &[]);
    cache.stored_inputs = vec![StoredInput {
        id: StoredInputId(0),
        value: StoredValue::Atom(a(0)),
    }];
    let mut call = request(1, &[], &[]);
    call.kind = LoweringKind(6);
    call.effects.call = Some(
        ProcedureAbi::new(CallingConvention::SameContext, 0, 0)
            .unwrap()
            .invocation(Invocation::Exec)
            .unwrap(),
    );
    let params: Vec<_> = (0..20).collect();
    let fixture = source(
        vec![
            segment(0, &params, vec![cache]),
            segment(1, &[], vec![call]),
            segment(2, &[], vec![]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[]),
            edge(1, 1, Some(2), EdgeKind::Forward, &[]),
            edge(
                2,
                2,
                None,
                EdgeKind::Return,
                &params.iter().map(|v| (*v, *v)).collect::<Vec<_>>(),
            ),
        ],
        seq(vec![s(0), e(0), s(1), e(1), s(2), e(2)]),
        20,
    );
    let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
    for id in [1, 2] {
        assert_eq!(
            plan.contracts[&SegmentId(id)].stack_top_first,
            (0..20).map(a).collect::<Vec<_>>()
        );
        assert!(
            plan.contracts[&SegmentId(id)]
                .required_spill_slots
                .contains(&(SpillSlotId::canonical(a(0)), a(0)))
        );
    }
    assert!(plan.edge_actions[&EdgeId(0)].is_empty());
    assert!(plan.edge_actions[&EdgeId(1)].is_empty());
    assert_eq!(
        plan.segment_actions[&SegmentId(1)],
        vec![PlanAction::RunLowering {
            step: SelectedStepId(1)
        }]
    );
    let args: Vec<_> = (100..120).collect();
    execute(&plan, &args, &args);
}

#[test]
fn context_entry_cleanup_and_selected_return_normalization_are_planned_explicitly() {
    for arguments in 0..=16 {
        let params: Vec<_> = (0..arguments).collect();
        let mut fixture = source(
            vec![segment(0, &params, vec![])],
            vec![edge(
                0,
                0,
                None,
                EdgeKind::Return,
                &params.iter().map(|v| (*v, *v)).collect::<Vec<_>>(),
            )],
            seq(vec![s(0), e(0)]),
            arguments,
        );
        fixture.0.abi =
            ProcedureAbi::new(CallingConvention::Context, arguments, arguments).unwrap();
        let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
        assert_eq!(plan.contracts[&SegmentId(0)].tail, TailPolicy::ContextWindow);
        let frame = frame(&plan);
        verify(&plan, &frame, body(&plan, &frame)).unwrap();
    }
    let mut literal = request(0, &[], &(100..116).collect::<Vec<_>>());
    literal.kind = LoweringKind(2);
    let mut cleanup = request(1, &(100..116).collect::<Vec<_>>(), &(200..216).collect::<Vec<_>>());
    cleanup.kind = LoweringKind(10);
    let mut fixture = source(
        vec![segment(0, &[], vec![literal, cleanup])],
        vec![edge(
            0,
            0,
            None,
            EdgeKind::Return,
            &(200..216).map(|v| (v, v)).collect::<Vec<_>>(),
        )],
        seq(vec![s(0), e(0)]),
        16,
    );
    fixture.0.abi = ProcedureAbi::new(CallingConvention::Context, 0, 16).unwrap();
    for index in 0..16 {
        fixture.0.values.get_mut(&ValueId(100 + index)).unwrap().reconstruction =
            Some(Reconstruction::Literal(vec![miden_core::Felt::new_unchecked(u64::from(index))]));
        fixture.0.values.get_mut(&ValueId(200 + index)).unwrap().reconstruction =
            Some(Reconstruction::Alias(vec![a(100 + index)]));
    }
    let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
    let frame = frame(&plan);
    verify(&plan, &frame, body(&plan, &frame)).unwrap();
    let mut missing = plan.clone();
    missing.segment_actions.get_mut(&SegmentId(0)).unwrap().retain(|action| {
        *action
            != PlanAction::RunLowering {
                step: SelectedStepId(1),
            }
    });
    assert!(verify(&missing, &frame, body(&missing, &frame)).is_err());
}

fn scratch_source(possible: bool, exact: bool) -> Source {
    let object = ScratchRequirement {
        id: ScratchId(0),
        size: 1,
        alignment: 1,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: ScratchLifetime::Spanning,
        uses: vec![SelectedStepId(0), SelectedStepId(1), SelectedStepId(2)],
    };
    let mut writer = request(0, &[0], &[]);
    writer.kind = LoweringKind(7);
    writer.scratch = vec![object.clone()];
    writer.scratch_effects = vec![ScratchEffect::Write(ScratchCellFact {
        object: ScratchId(0),
        offset: 0,
        content: ContentRequirement::Atom(a(0)),
    })];
    let mut maybe = request(1, &[], &[]);
    maybe.kind = LoweringKind(9);
    maybe.scratch = vec![object.clone()];
    if possible {
        maybe.scratch_effects = vec![ScratchEffect::PossibleWrite {
            object: ScratchId(0),
            offset: 0,
            size: 1,
        }];
    }
    let mut reader = request(2, &[], &[100]);
    reader.kind = LoweringKind(8);
    reader.scratch = vec![object];
    reader.scratch_effects = vec![ScratchEffect::Read(ScratchCellFact {
        object: ScratchId(0),
        offset: 0,
        content: if exact {
            ContentRequirement::Atom(a(0))
        } else {
            ContentRequirement::Initialized
        },
    })];
    source(
        vec![segment(0, &[0], vec![writer, maybe]), segment(1, &[], vec![reader])],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[]),
            edge(1, 1, None, EdgeKind::Return, &[(100, 100)]),
        ],
        seq(vec![s(0), e(0), s(1), e(1)]),
        1,
    )
}

#[test]
fn scratch_completion_retains_initialization_but_forgets_possible_write_identity() {
    for (possible, exact) in [(false, true), (true, false)] {
        let fixture = scratch_source(possible, exact);
        let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
        let frame = frame(&plan);
        let ready = bound(&plan, &frame);
        verify(&ready, &frame, body(&ready, &frame)).unwrap();
    }
    let impossible = scratch_source(true, true);
    assert!(allocate(checked(&impossible), &AllocationOptions::default()).is_err());
}

#[test]
fn terminal_bindings_use_source_order_without_destination_substitution() {
    let fixture = source(
        vec![segment(0, &[0, 1], vec![])],
        vec![edge(0, 0, None, EdgeKind::Return, &[(0, 1), (1, 0)])],
        seq(vec![s(0), e(0)]),
        2,
    );
    let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
    execute(&plan, &[101, 202], &[202, 101]);
}

#[test]
fn loop_scratch_content_rebinding_requires_the_new_parameter_token() {
    let object = ScratchRequirement {
        id: ScratchId(0),
        size: 1,
        alignment: 1,
        contiguous: true,
        address_space: AddressSpace::ProcedureLocal,
        lifetime: ScratchLifetime::Spanning,
        uses: vec![SelectedStepId(0), SelectedStepId(2), SelectedStepId(4)],
    };
    let fact = |content| ScratchCellFact {
        object: ScratchId(0),
        offset: 0,
        content,
    };
    let mut initial = request(0, &[0], &[]);
    initial.kind = LoweringKind(7);
    initial.scratch = vec![object.clone()];
    initial.scratch_effects = vec![
        ScratchEffect::Write(fact(ContentRequirement::Content(ContentId(0)))),
        ScratchEffect::Write(fact(ContentRequirement::Atom(a(0)))),
    ];
    let mut read = request(2, &[], &[50]);
    read.kind = LoweringKind(8);
    read.scratch = vec![object.clone()];
    read.scratch_effects = vec![
        ScratchEffect::Read(fact(ContentRequirement::Content(ContentId(1)))),
        ScratchEffect::Read(fact(ContentRequirement::Atom(a(10)))),
    ];
    let mut compare = request(1, &[50, 2], &[20]);
    compare.kind = LoweringKind(4);
    let mut next = request(3, &[10, 1], &[30]);
    next.kind = LoweringKind(3);
    let mut write = request(4, &[30], &[]);
    write.kind = LoweringKind(7);
    write.scratch = vec![object];
    write.scratch_effects = vec![
        ScratchEffect::Write(fact(ContentRequirement::Content(ContentId(2)))),
        ScratchEffect::Write(fact(ContentRequirement::Atom(a(30)))),
    ];
    let loop_tree = ControlTree::Loop {
        id: ControlId(0),
        condition: ControlId(1),
        header: SegmentId(1),
        predicate: a(20),
        on_true: EdgeId(1),
        on_false: EdgeId(2),
        body: alloc::boxed::Box::new(seq(vec![s(2), e(3)])),
        backedges: vec![EdgeId(3)],
    };
    let mut fixture = source(
        vec![
            segment(0, &[0, 1, 2], vec![initial]),
            segment(1, &[10], vec![read, compare]),
            segment(2, &[], vec![next, write]),
            segment(3, &[40], vec![]),
        ],
        vec![
            edge(0, 0, Some(1), EdgeKind::Forward, &[(10, 0)]),
            edge(1, 1, Some(2), EdgeKind::True, &[]),
            edge(2, 1, Some(3), EdgeKind::False, &[(40, 10)]),
            edge(3, 2, Some(1), EdgeKind::Backedge, &[(10, 30)]),
            edge(4, 3, None, EdgeKind::Return, &[(40, 40)]),
        ],
        seq(vec![s(0), e(0), loop_tree, s(3), e(4)]),
        1,
    );
    fixture.0.segments[1].content_parameters = vec![ContentId(1)];
    fixture.0.edges[0].content_bindings = vec![ContentBinding {
        destination: ContentId(1),
        source: ContentId(0),
    }];
    fixture.0.edges[3].content_bindings = vec![ContentBinding {
        destination: ContentId(1),
        source: ContentId(2),
    }];
    fixture.0.values.get_mut(&ValueId(50)).unwrap().reconstruction =
        Some(Reconstruction::Alias(vec![a(10)]));
    let plan = allocate(checked(&fixture), &AllocationOptions::default()).unwrap();
    assert!(
        plan.contracts[&SegmentId(1)]
            .required_scratch
            .contains(&fact(ContentRequirement::Atom(a(10))))
    );
    execute(&plan, &[0, 1, 3], &[3]);
    execute(&plan, &[5, 1, 5], &[5]);
    let frame = frame(&plan);
    let ready = bound(&plan, &frame);
    fn corrupt(body: &AnnotatedMasmBody, offset: u16) -> AnnotatedMasmBody {
        let mut out = AnnotatedMasmBody::new();
        for op in body.ops() {
            match op {
                AnnotatedOp::Instruction {
                    instruction: Instruction::Locaddr(_),
                    unit,
                } if *unit == SelectedStepId(4) => out.push_instruction(Instruction::Drop, *unit),
                AnnotatedOp::Instruction {
                    instruction: Instruction::MemStore,
                    unit,
                } if *unit == SelectedStepId(4) => {
                    // Preserve stack shape and perform a real write, but write the old cell token.
                    for instruction in [
                        Instruction::Locaddr(offset.into()),
                        Instruction::MemLoad,
                        Instruction::Locaddr(offset.into()),
                        Instruction::MemStore,
                    ] {
                        out.push_instruction(instruction, *unit);
                    }
                }
                AnnotatedOp::If {
                    id,
                    then_body,
                    else_body,
                } => out.push(AnnotatedOp::If {
                    id: *id,
                    then_body: corrupt(then_body, offset),
                    else_body: corrupt(else_body, offset),
                }),
                AnnotatedOp::While { id, body } => out.push(AnnotatedOp::While {
                    id: *id,
                    body: corrupt(body, offset),
                }),
                other => out.push(other.clone()),
            }
        }
        out
    }
    let offset = frame
        .resolve(FrameRef::Scratch {
            object: ScratchId(0),
            offset: 0,
        })
        .unwrap();
    assert_eq!(
        verify(&ready, &frame, corrupt(&body(&ready, &frame), offset))
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidSpillSlotRead
    );
}

#[test]
fn procedure_allocator_consumes_checked_units_and_repairs_return() {
    let definition = DiscardDefinition {
        types: vec![midenc_hir::Type::Felt; 20],
        requests: vec![unit(vec![Atom::new(ValueId(19), 0)], vec![])],
    };
    let selected = check_selection(
        definition.instantiate(&[]).unwrap().program,
        &SourceContext::Generated(&definition),
    )
    .unwrap();
    let plan = allocate(selected, &AllocationOptions::default()).unwrap();
    assert_eq!(plan.contracts[&SegmentId(0)].stack_top_first.len(), 20);
    let frame = fixture_frame(&plan);
    verify(&plan, &frame, fixture_body(&plan, &frame)).unwrap();
}
