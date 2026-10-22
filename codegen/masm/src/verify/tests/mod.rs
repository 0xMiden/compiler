mod abi;
mod contents;
mod control;
mod dataflow;
mod diagnostics;
mod diamond;
mod frame;

pub(super) mod loops;
mod memory;
mod mutations;
mod provenance;
mod reconstruction;
mod selection;
mod storage_writes;
mod tokens;
use alloc::{collections::BTreeMap, vec, vec::Vec};

use miden_core::Felt;
use midenc_hir::Type;

use super::*;
use crate::{
    lower::{expand::*, units::LoweringBindings},
    masm::Instruction,
    plan::actions::*,
};

struct Identity;

impl GeneratedDefinition for Identity {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(0)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        assert!(parameters.is_empty());
        let atom = Atom::new(ValueId(0), 0);
        let result = Atom::new(ValueId(1), 0);
        Ok(DefinitionEvidence {
            source_locals: SourceLocalLayout::default(),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values: BTreeMap::from([
                    (
                        ValueId(0),
                        SelectedValue {
                            id: ValueId(0),
                            origin: ValueOrigin::Parameter {
                                segment: SegmentId(0),
                                index: 0,
                            },
                            layout: ValueLayout::new(Type::Felt).unwrap(),
                            reconstruction: None,
                        },
                    ),
                    (
                        ValueId(1),
                        SelectedValue {
                            id: ValueId(1),
                            origin: ValueOrigin::Temporary {
                                step: SourceStepId(0),
                                index: 0,
                            },
                            layout: ValueLayout::new(Type::Felt).unwrap(),
                            reconstruction: None,
                        },
                    ),
                ]),
                segments: vec![SelectedSegment {
                    content_parameters: vec![],
                    id: SegmentId(0),
                    parameters: vec![ValueId(0)],
                    units: vec![SelectedLowering {
                        address_results: vec![],
                        id: SelectedStepId(0),
                        kind: LoweringKind(0),
                        source: SourceStepId(0),
                        semantic_inputs: vec![ValueId(0)],
                        stack_inputs: vec![atom],
                        stored_inputs: vec![],
                        outputs: vec![result],
                        effects: LoweringEffects::default(),
                        scratch: vec![],
                        scratch_effects: vec![],
                    }],
                }],
                edges: vec![TransferEdge {
                    content_bindings: vec![],
                    id: EdgeId(0),
                    source: SegmentId(0),
                    destination: None,
                    kind: EdgeKind::Return,
                    bindings: vec![ParameterBinding {
                        destination: ValueId(1),
                        source: ValueId(1),
                    }],
                }],
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                ]),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 1, 1).unwrap(),
                source_map: BTreeMap::from([(
                    SourceStepId(0),
                    SourceRecord {
                        origin: SourceOrigin::Generated {
                            definition: self.id(),
                            stage: 0,
                        },
                        location: SourceLocation::default(),
                    },
                )]),
            },
            units: BTreeMap::from([(
                SelectedStepId(0),
                UnitEvidence {
                    aliases: LaneAliases::new(vec![(result, atom)]),
                    ..Default::default()
                },
            )]),
        })
    }
}

fn identity_plan() -> ProcedurePlan {
    let candidate = Identity.instantiate(&[]).unwrap().program;
    let selected = check_selection(candidate, &SourceContext::Generated(&Identity)).unwrap();
    ProcedurePlan {
        debug_locations: Default::default(),
        decisions: Default::default(),
        selected,
        contracts: BTreeMap::from([(
            SegmentId(0),
            EntryContract {
                stack_top_first: vec![Atom::new(ValueId(0), 0)],
                required_spill_slots: vec![],
                required_scratch: vec![],
                address_facts: vec![],
                aliases: LaneAliases::default(),
                tail: TailPolicy::PreserveSuffix,
            },
        )]),
        segment_actions: BTreeMap::from([(
            SegmentId(0),
            vec![PlanAction::RunLowering {
                step: SelectedStepId(0),
            }],
        )]),
        edge_actions: BTreeMap::new(),
        spill_slots: vec![],
        lowering_bindings: BTreeMap::from([(SelectedStepId(0), LoweringBindings::default())]),
    }
}

fn body(instructions: Vec<Instruction>) -> AnnotatedMasmBody {
    let mut body = AnnotatedMasmBody::new();
    body.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
    body.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
    for instruction in instructions {
        body.push_instruction(instruction, SelectedStepId(0));
    }
    body.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
    body.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
    body
}

fn frame() -> FinalFrameLayout {
    FinalFrameLayout::new(SourceLocalLayout::default(), BTreeMap::new(), BTreeMap::new(), 0)
        .unwrap()
}

#[test]
fn alias_output_must_equal_input() {
    assert!(verify(&identity_plan(), &frame(), body(vec![])).is_ok());
    let error = verify(
        &identity_plan(),
        &frame(),
        body(vec![Instruction::Drop, Primitive::Push(Felt::ONE).instruction().unwrap()]),
    )
    .unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::UnavailableValue);
}

#[test]
fn publication_cannot_rename_suffix() {
    let error = verify(&identity_plan(), &frame(), body(vec![Instruction::Swap1])).unwrap_err();
    assert_eq!(error.category, DiagnosticCategory::IllegalStackAccess);
}

#[test]
fn omitted_effectful_unit_fails_before_replay() {
    let mut candidate = Identity.instantiate(&[]).unwrap().program;
    candidate.segments[0].units.clear();
    assert_eq!(
        check_selection(candidate, &SourceContext::Generated(&Identity))
            .unwrap_err()
            .category,
        DiagnosticCategory::InvalidSelectedLowering
    );
}

/// A registered test definition reconstructs its single semantic stage from the
/// declared signature and requirements. It never receives the candidate program.
#[derive(Clone)]
struct Fixture {
    locals: SourceLocalLayout,
    inputs: u32,
    outputs: u32,
    proof: UnitEvidence,
    scratch: Vec<ScratchRequirement>,
    scratch_effects: Vec<ScratchEffect>,
    effects: LoweringEffects,
    stored: Vec<StoredInput>,
    convention: CallingConvention,
}

impl Fixture {
    fn new(inputs: u32, outputs: u32) -> Self {
        Self {
            locals: SourceLocalLayout::default(),
            inputs,
            outputs,
            proof: UnitEvidence::default(),
            scratch: vec![],
            scratch_effects: vec![],
            effects: LoweringEffects::default(),
            stored: vec![],
            convention: CallingConvention::SameContext,
        }
    }

    fn input(index: u32) -> Atom {
        Atom::new(ValueId(index), 0)
    }

    fn output(index: u32) -> Atom {
        Atom::new(ValueId(100 + index), 0)
    }

    fn scratch(mut self, size: u32) -> Self {
        self.scratch.push(ScratchRequirement {
            id: ScratchId(0),
            size,
            alignment: 1,
            contiguous: true,
            address_space: AddressSpace::ProcedureLocal,
            lifetime: ScratchLifetime::Unit(SelectedStepId(0)),
            uses: vec![SelectedStepId(0)],
        });
        self
    }

    fn plan(&self) -> ProcedurePlan {
        let selected = check_selection(
            self.instantiate(&[]).unwrap().program,
            &SourceContext::Generated(self),
        )
        .unwrap();
        ProcedurePlan {
            debug_locations: Default::default(),
            decisions: Default::default(),
            selected,
            contracts: BTreeMap::from([(
                SegmentId(0),
                EntryContract {
                    stack_top_first: (0..self.inputs).map(Self::input).collect(),
                    required_spill_slots: vec![],
                    required_scratch: vec![],
                    address_facts: vec![],
                    aliases: LaneAliases::default(),
                    tail: TailPolicy::PreserveSuffix,
                },
            )]),
            segment_actions: BTreeMap::from([(
                SegmentId(0),
                vec![PlanAction::RunLowering {
                    step: SelectedStepId(0),
                }],
            )]),
            edge_actions: BTreeMap::new(),
            spill_slots: vec![],
            lowering_bindings: BTreeMap::from([(
                SelectedStepId(0),
                LoweringBindings {
                    stored_inputs: BTreeMap::new(),
                    scratch: self.allocations(),
                },
            )]),
        }
    }

    fn allocations(&self) -> BTreeMap<ScratchId, ScratchAllocation> {
        let mut offset = self.locals.extent;
        self.scratch
            .iter()
            .map(|requirement| {
                let allocation = ScratchAllocation {
                    offset,
                    size: requirement.size,
                    alignment: requirement.alignment,
                    lifetime: requirement.lifetime,
                };
                offset += requirement.size;
                (requirement.id, allocation)
            })
            .collect()
    }

    fn frame(&self) -> FinalFrameLayout {
        let size = self.locals.extent + self.scratch.iter().map(|object| object.size).sum::<u32>();
        FinalFrameLayout::new(
            self.locals.clone(),
            BTreeMap::new(),
            self.allocations(),
            size.next_multiple_of(4),
        )
        .unwrap()
    }

    fn run(&self, code: Vec<AnnotatedOp>) -> Result<VerifiedProcedure, VerificationError> {
        let mut result = AnnotatedMasmBody::new();
        result.push(AnnotatedOp::Boundary(Boundary::Segment(SegmentId(0))));
        result.push(AnnotatedOp::Boundary(Boundary::UnitStart(SelectedStepId(0))));
        for op in code {
            result.push(op);
        }
        result.push(AnnotatedOp::Boundary(Boundary::UnitEnd(SelectedStepId(0))));
        result.push(AnnotatedOp::Boundary(Boundary::Edge(EdgeId(0))));
        verify(&self.plan(), &self.frame(), result)
    }
}

impl GeneratedDefinition for Fixture {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(1)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() {
            return Err(error("fixture definition takes no parameters"));
        }
        let mut values = BTreeMap::new();
        for input in 0..self.inputs {
            values.insert(
                ValueId(input),
                SelectedValue {
                    id: ValueId(input),
                    origin: ValueOrigin::Parameter {
                        segment: SegmentId(0),
                        index: input,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
        }
        for output in 0..self.outputs {
            values.insert(
                ValueId(100 + output),
                SelectedValue {
                    id: ValueId(100 + output),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(0),
                        index: output,
                    },
                    layout: ValueLayout::new(Type::Felt).unwrap(),
                    reconstruction: None,
                },
            );
        }
        Ok(DefinitionEvidence {
            source_locals: self.locals.clone(),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments: vec![SelectedSegment {
                    content_parameters: vec![],
                    id: SegmentId(0),
                    parameters: (0..self.inputs).map(ValueId).collect(),
                    units: vec![SelectedLowering {
                        address_results: vec![],
                        id: SelectedStepId(0),
                        kind: LoweringKind(0),
                        source: SourceStepId(0),
                        semantic_inputs: (0..self.inputs).map(ValueId).collect(),
                        stack_inputs: (0..self.inputs).map(Self::input).collect(),
                        stored_inputs: self.stored.clone(),
                        outputs: (0..self.outputs).map(Self::output).collect(),
                        effects: self.effects.clone(),
                        scratch: self.scratch.clone(),
                        scratch_effects: self.scratch_effects.clone(),
                    }],
                }],
                edges: vec![TransferEdge {
                    content_bindings: vec![],
                    id: EdgeId(0),
                    source: SegmentId(0),
                    destination: None,
                    kind: EdgeKind::Return,
                    bindings: (0..self.outputs)
                        .map(|output| ParameterBinding {
                            destination: ValueId(100 + output),
                            source: ValueId(100 + output),
                        })
                        .collect(),
                }],
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                ]),
                abi: ProcedureAbi::new(self.convention, self.inputs, self.outputs).unwrap(),
                source_map: BTreeMap::from([(
                    SourceStepId(0),
                    SourceRecord {
                        origin: SourceOrigin::Generated {
                            definition: self.id(),
                            stage: 0,
                        },
                        location: SourceLocation::default(),
                    },
                )]),
            },
            units: BTreeMap::from([(SelectedStepId(0), self.proof.clone())]),
        })
    }
}

pub(crate) fn empty_program() -> CheckedSelectedProgram {
    struct Empty;
    impl GeneratedDefinition for Empty {
        fn id(&self) -> GeneratedDefinitionId {
            GeneratedDefinitionId(8)
        }

        fn instantiate(
            &self,
            parameters: &[Felt],
        ) -> Result<DefinitionEvidence, VerificationError> {
            let mut evidence = Fixture::new(0, 0).instantiate(parameters)?;
            evidence.program.origin = ProcedureOrigin::Generated {
                definition: self.id(),
                parameters: vec![],
            };
            evidence.program.segments[0].units.clear();
            evidence.program.source_map.clear();
            evidence.units.clear();
            Ok(evidence)
        }
    }
    check_selection(Empty.instantiate(&[]).unwrap().program, &SourceContext::Generated(&Empty))
        .unwrap()
}

fn inst(instruction: Instruction) -> AnnotatedOp {
    AnnotatedOp::Instruction {
        instruction,
        unit: SelectedStepId(0),
    }
}

fn push(value: u64) -> AnnotatedOp {
    inst(Primitive::Push(Felt::new_unchecked(value)).instruction().unwrap())
}

fn ops(code: Vec<AnnotatedOp>) -> AnnotatedMasmBody {
    let mut body = AnnotatedMasmBody::new();
    for op in code {
        body.push(op);
    }
    body
}
