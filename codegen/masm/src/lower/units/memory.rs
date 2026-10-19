use alloc::{collections::BTreeMap, vec};

use midenc_hir::Report;

use super::*;
use crate::{
    lower::{expand::annotate_raw, raw},
    verify::{CallArgument, CallPath, CallSite, LoopPosition, LoopSchema, UnitEvidence},
};

/// Registered overlap-safe byte copy with checked range arithmetic.
/// The raw algorithm uses an aligned element-copy helper or a directional byte loop.
pub(crate) struct ByteMemmove;

impl ByteMemmove {
    /// Require `[src, dst, count]` byte lanes and derive read/write, trap, loop, and call evidence.
    pub fn select(
        &self,
        id: SelectedStepId,
        source: SourceStepId,
        inputs: &[Atom],
    ) -> Result<(SelectedLowering, UnitEvidence), PlanError> {
        if inputs.len() != 3 {
            return Err(invalid("byte memmove requires src, dst, count"));
        }
        let mut unit = selected(self.kind(), id, source, inputs, &[], &[]);
        unit.effects = LoweringEffects {
            memory: SourceMemoryEffects::READ_WRITE,
            may_trap: true,
            normal_return: true,
            call: Some(
                ProcedureAbi::new(CallingConvention::SameContext, 3, 0)?
                    .invocation(Invocation::Exec)?,
            ),
        };
        let proof = UnitEvidence {
            loops: BTreeMap::from([(
                ControlId(2),
                LoopSchema {
                    stack: vec![
                        LoopPosition::Carry(0),
                        LoopPosition::Invariant,
                        LoopPosition::Invariant,
                        LoopPosition::Invariant,
                        LoopPosition::Invariant,
                    ],
                    cells: vec![],
                },
            )]),
            calls: vec![CallSite {
                instruction: masm::Instruction::Exec(raw::target(
                    "::intrinsics::mem::memmove_elements",
                )),
                abi: ProcedureAbi::new(CallingConvention::SameContext, 3, 0)?,
                inputs: vec![CallArgument::Local; 3],
                path: vec![CallPath::Then(ControlId(0))],
                truncate: false,
                outputs: vec![],
                publications: vec![],
                selector: None,
            }],
            ..Default::default()
        };
        Ok((unit, proof))
    }
}

impl LoweringDefinition for ByteMemmove {
    fn kind(&self) -> LoweringKind {
        LoweringKind(1000)
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv<'_>,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        Ok(annotate_raw(&raw::byte_memmove(Default::default()), unit.id))
    }
}

/// Explicit one-cell spanning storage phases. Content identities cross selected boundaries;
/// expansion only consumes a final object binding and emits the scalar memory instruction.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SpanningCell {
    /// Consume one lane and establish the declared content identity in the scratch cell.
    Write,
    /// Read the declared content identity and publish it as the single result lane.
    Read,
}
impl SpanningCell {
    /// Declare one phase of a one-element spanning object, including its content obligation.
    /// Reject unit-local lifetimes, non-scalar objects, and mismatched input/result counts.
    pub fn select(
        self,
        id: SelectedStepId,
        source: SourceStepId,
        object: &ScratchRequirement,
        content: ContentId,
        inputs: &[Atom],
        outputs: &[Atom],
    ) -> Result<(SelectedLowering, UnitEvidence), PlanError> {
        let write = matches!(self, Self::Write);
        if object.lifetime != ScratchLifetime::Spanning
            || object.size != 1
            || inputs.len() != usize::from(write)
            || outputs.len() != usize::from(!write)
        {
            return Err(invalid("spanning scalar phase contract mismatch"));
        }
        let mut unit = selected(self.kind(), id, source, inputs, &[], outputs);
        unit.scratch = vec![object.clone()];
        let fact = ScratchCellFact {
            object: object.id,
            offset: 0,
            content: ContentRequirement::Content(content),
        };
        unit.scratch_effects = vec![if write {
            ScratchEffect::Write(fact)
        } else {
            ScratchEffect::Read(fact)
        }];
        let proof = UnitEvidence {
            scratch_outputs: if write {
                vec![]
            } else {
                vec![(
                    FrameRef::Scratch {
                        object: object.id,
                        offset: 0,
                    },
                    0,
                )]
            },
            ..Default::default()
        };
        Ok((unit, proof))
    }
}
impl LoweringDefinition for SpanningCell {
    fn kind(&self) -> LoweringKind {
        LoweringKind(if matches!(self, Self::Write) {
            1204
        } else {
            1205
        })
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv<'_>,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        let mut body = AnnotatedMasmBody::new();
        body.push_instruction(
            masm::Instruction::Locaddr(
                frame
                    .resolve(FrameRef::Scratch {
                        object: unit.scratch[0].id,
                        offset: 0,
                    })?
                    .into(),
            ),
            unit.id,
        );
        body.push_instruction(
            if matches!(self, Self::Write) {
                masm::Instruction::MemStore
            } else {
                masm::Instruction::MemLoad
            },
            unit.id,
        );
        Ok(body)
    }
}
