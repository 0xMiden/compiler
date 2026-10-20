//! Source authority for compiler-generated root procedures.
//!
//! Generated roots have no HIR body to reconstruct. Their registered definition instead derives
//! the selected graph, call evidence, and executable stages from the same semantic parameters.
//! Stage order and the final VM stack cleanup are part of that definition's contract.
use alloc::{collections::BTreeMap, vec, vec::Vec};

use miden_core::Felt;
use midenc_hir::{Report, Type};

use crate::{
    lower::{
        expand::{AnnotatedMasmBody, ExpansionEnv, annotate_raw},
        raw,
        units::{LoweringBindings, LoweringDefinition},
    },
    masm::{self, Instruction},
    plan::*,
    verify::{
        CallArgument, CallSite, DefinitionEvidence, GeneratedDefinition, UnitEvidence,
        VerificationError,
    },
};

/// Semantic inputs for a root wrapper which initializes, invokes an entrypoint, and cleans up.
/// Implements source reconstruction through `GeneratedDefinition`; `definitions()` supplies
/// the corresponding executable lowerings for registration before expansion.
pub(crate) struct GeneratedRoot {
    /// Optional initialization called before the harness and entrypoint, with no ABI lanes.
    pub initializer: Option<masm::InvocationTarget>,
    /// Optional test harness setup, also called with no ABI lanes.
    pub harness: Option<masm::InvocationTarget>,
    /// Entrypoint invoked after any setup stages.
    pub entry: masm::InvocationTarget,
    /// Entrypoint argument/result counts; root selection requires at most sixteen arguments.
    pub entry_abi: ProcedureAbi,
}

/// One closed executable stage of a [`GeneratedRoot`].
/// Its private stage identity fixes both the lowering kind and the matching generated source step.
pub(crate) struct RootStage {
    stage: u32,
    target: masm::InvocationTarget,
    abi: ProcedureAbi,
}

impl GeneratedRoot {
    /// Construct present setup stages, the entry call, and mandatory stack truncation in order.
    /// Stage identities remain fixed when optional setup stages are absent.
    pub fn definitions(&self) -> Vec<RootStage> {
        let mut stages = Vec::new();
        for (stage, target) in [
            (0, self.initializer.as_ref()),
            (1, self.harness.as_ref()),
            (2, Some(&self.entry)),
        ] {
            if let Some(target) = target {
                stages.push(RootStage {
                    stage,
                    target: target.clone(),
                    abi: if stage == 2 {
                        self.entry_abi
                    } else {
                        ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap()
                    },
                });
            }
        }
        stages.push(RootStage {
            stage: 3,
            target: raw::target("::miden::core::sys::truncate_stack"),
            abi: ProcedureAbi::new(CallingConvention::SameContext, 0, 0).unwrap(),
        });
        stages
    }
}

impl GeneratedDefinition for GeneratedRoot {
    fn id(&self) -> GeneratedDefinitionId {
        GeneratedDefinitionId(1000)
    }

    fn instantiate(&self, parameters: &[Felt]) -> Result<DefinitionEvidence, VerificationError> {
        if !parameters.is_empty() || self.entry_abi.arguments() > 16 {
            return Err(PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "root parameters exceed the supported input window",
            ));
        }
        self.entry_abi.invocation(Invocation::Exec)?;
        // Inputs, entry results and cleanup results occupy consecutive disjoint ranges.
        // Check the entire range before constructing identities or narrowing host indices.
        let entry_base = 16u32;
        let cleanup_base = entry_base
            .checked_add(self.entry_abi.results())
            .filter(|base| base.checked_add(16).is_some())
            .ok_or_else(|| {
                PlanError::new(
                    DiagnosticCategory::InvalidSelectedLowering,
                    "generated root value identity range overflows",
                )
            })?;
        let atom = |i| Atom::new(ValueId(i), 0);
        let mut values = BTreeMap::new();
        for i in 0..16 {
            values.insert(
                ValueId(i),
                SelectedValue {
                    id: ValueId(i),
                    origin: ValueOrigin::Parameter {
                        segment: SegmentId(0),
                        index: i,
                    },
                    layout: ValueLayout::new(Type::Felt)?,
                    reconstruction: None,
                },
            );
        }
        for i in 0..self.entry_abi.results() {
            values.insert(
                ValueId(entry_base + i),
                SelectedValue {
                    id: ValueId(entry_base + i),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(2),
                        index: i,
                    },
                    layout: ValueLayout::new(Type::Felt)?,
                    reconstruction: None,
                },
            );
        }
        let after = (0..self.entry_abi.results())
            .map(|i| atom(entry_base + i))
            .chain((self.entry_abi.arguments()..16).map(atom))
            .collect::<Vec<_>>();
        for i in 0..16 {
            values.insert(
                ValueId(cleanup_base + i),
                SelectedValue {
                    id: ValueId(cleanup_base + i),
                    origin: ValueOrigin::Temporary {
                        step: SourceStepId(3),
                        index: i,
                    },
                    layout: ValueLayout::new(Type::Felt)?,
                    reconstruction: if usize::try_from(i).is_ok_and(|index| index >= after.len()) {
                        Some(Reconstruction::Literal(vec![Felt::ZERO]))
                    } else {
                        None
                    },
                },
            );
        }
        let mut units = Vec::new();
        let mut proofs = BTreeMap::new();
        let mut source_map = BTreeMap::new();
        for definition in self.definitions() {
            let id = SelectedStepId(definition.stage);
            let source = SourceStepId(definition.stage);
            let inputs = match definition.stage {
                2 => (0..self.entry_abi.arguments()).map(atom).collect::<Vec<_>>(),
                3 => after.clone(),
                _ => vec![],
            };
            let outputs = match definition.stage {
                2 => {
                    (0..self.entry_abi.results()).map(|i| atom(entry_base + i)).collect::<Vec<_>>()
                }
                3 => (0..16).map(|i| atom(cleanup_base + i)).collect::<Vec<_>>(),
                _ => vec![],
            };
            let cleanup = definition.stage == 3;
            let aliases = if cleanup {
                LaneAliases::new(outputs.iter().zip(&inputs).map(|(o, i)| (*o, *i)).collect())
            } else {
                LaneAliases::default()
            };
            let mut call_inputs = inputs
                .iter()
                .map(|a| CallArgument::Exact(StoredValue::Atom(*a)))
                .collect::<Vec<_>>();
            if cleanup {
                call_inputs.clear();
            }
            let proof = UnitEvidence {
                aliases,
                calls: vec![CallSite {
                    instruction: Instruction::Exec(definition.target.clone()),
                    abi: definition.abi,
                    inputs: call_inputs,
                    path: vec![],
                    truncate: cleanup,
                    outputs: vec![],
                    publications: if cleanup {
                        vec![]
                    } else {
                        outputs.iter().zip(0u32..).map(|(a, i)| (*a, i)).collect()
                    },
                    selector: None,
                }],
                ..Default::default()
            };
            let unit = SelectedLowering {
                id,
                kind: definition.kind(),
                source,
                semantic_inputs: inputs.iter().map(|a| a.value).collect(),
                stack_inputs: inputs,
                stored_inputs: vec![],
                outputs,
                address_results: vec![],
                effects: LoweringEffects {
                    memory: if definition.stage < 2 {
                        SourceMemoryEffects::WRITE
                    } else {
                        SourceMemoryEffects::default()
                    },
                    ..Default::default()
                },
                scratch: vec![],
                scratch_effects: vec![],
            };
            source_map.insert(
                source,
                SourceRecord {
                    origin: SourceOrigin::Generated {
                        definition: self.id(),
                        stage: definition.stage,
                    },
                    location: Default::default(),
                },
            );
            proofs.insert(id, proof);
            units.push(unit);
        }
        Ok(DefinitionEvidence {
            source_locals: Default::default(),
            program: SelectedProgram {
                origin: ProcedureOrigin::Generated {
                    definition: self.id(),
                    parameters: vec![],
                },
                values,
                segments: vec![SelectedSegment {
                    id: SegmentId(0),
                    parameters: (0..16).map(ValueId).collect(),
                    content_parameters: vec![],
                    units,
                }],
                edges: vec![TransferEdge {
                    id: EdgeId(0),
                    source: SegmentId(0),
                    destination: None,
                    kind: EdgeKind::Return,
                    bindings: (0..16)
                        .map(|i| ParameterBinding {
                            destination: ValueId(cleanup_base + i),
                            source: ValueId(cleanup_base + i),
                        })
                        .collect(),
                    content_bindings: vec![],
                }],
                structure: ControlTree::Sequence(vec![
                    ControlTree::Segment(SegmentId(0)),
                    ControlTree::Edge(EdgeId(0)),
                ]),
                abi: ProcedureAbi::new(CallingConvention::Root, 16, 16)?,
                source_map,
            },
            units: proofs,
        })
    }
}

impl LoweringDefinition for RootStage {
    fn kind(&self) -> LoweringKind {
        LoweringKind(1100 + self.stage)
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv<'_>,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        let mut b = raw::Builder::new(Default::default());
        // Materialize only established VM floor zeros when the meaningful root state is short.
        // Truncation then preserves the same top sixteen and removes every overflow lane.
        if self.stage == 3 && unit.stack_inputs.len() < 16 {
            b.extend(raw::pad_inputs(unit.stack_inputs.len(), b.span));
        }
        b.exec(self.target.clone());
        Ok(annotate_raw(&b.ops, unit.id))
    }
}
