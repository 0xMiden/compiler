//! Turn a completed physical plan into annotated MASM, then verify its executable behavior.
//!
//! Allocation actions resolve through the final frame; semantic lowering actions invoke a
//! registered definition. Expansion cannot allocate or reschedule values. The resulting body
//! remains unverified through peephole optimization: only `finalize`'s verifier can turn it into
//! a procedure eligible for emission.
use alloc::vec::Vec;

use crate::{masm, plan::*};

/// Materialize bindings from the completed frame atomically, before any lowering expands.
/// Missing unit bindings or incompatible scratch assignments leave the plan unchanged.
pub(crate) fn resolve_frame_bindings(
    plan: &mut actions::ProcedurePlan,
    frame: &FinalFrameLayout,
) -> Result<(), PlanError> {
    let mut resolved = alloc::collections::BTreeMap::new();
    for unit in plan.selected.segments.iter().flat_map(|segment| &segment.units) {
        let binding = plan.lowering_bindings.get(&unit.id).ok_or_else(|| {
            PlanError::new(DiagnosticCategory::InvalidSelectedLowering, "missing lowering bindings")
        })?;
        resolved.insert(unit.id, binding.resolve_scratch(unit, frame)?);
    }
    plan.lowering_bindings = resolved;
    Ok(())
}

/// Expand an allocation action after symbolic frame references have been assigned.
/// Scalar local instructions expand in the assembler to `locaddr; mem_store/mem_load`.
/// Lowering execution has its own boundary and is deliberately not accepted here.
pub(crate) fn expand_allocation_action(
    action: &actions::PlanAction,
    frame: &FinalFrameLayout,
) -> Result<Option<masm::Instruction>, PlanError> {
    use actions::PlanAction;
    Ok(Some(match *action {
        PlanAction::Dup { depth } => Primitive::Dup(depth).instruction()?,
        PlanAction::Swap { depth } => Primitive::Swap(depth).instruction()?,
        PlanAction::MoveUp { depth } => Primitive::MoveUp(depth).instruction()?,
        PlanAction::MoveDown { depth } => Primitive::MoveDown(depth).instruction()?,
        PlanAction::Drop => masm::Instruction::Drop,
        PlanAction::PushLiteral(value) => Primitive::Push(value).instruction()?,
        PlanAction::StoreLane(slot) => {
            masm::Instruction::LocStore(frame.resolve(FrameRef::Spill(slot))?.into())
        }
        PlanAction::LoadLane(slot) => {
            masm::Instruction::LocLoad(frame.resolve(FrameRef::Spill(slot))?.into())
        }
        PlanAction::DebugLocation(_) => return Ok(None),
        PlanAction::RunLowering { .. } => {
            return Err(PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "lowering requires registered expansion and unit boundaries",
            ));
        }
    }))
}

/// Immutable metadata needed to expand already selected operations.
/// These lookups supply debug decorators and linked facts, never allocation decisions.
#[derive(Default)]
pub(crate) struct ExpansionEnv<'a> {
    /// Decorators keyed by the debug-location actions in the physical plan.
    pub debug: alloc::collections::BTreeMap<DebugLocationId, DebugDecorator>,
    /// Linked information available to definitions which require it.
    pub link_info: Option<&'a crate::linker::LinkInfo>,
}

impl ExpansionEnv<'_> {
    /// Resolve observational records against the same completed frame as executable accesses.
    /// This only populates inert decorators; it cannot allocate or modify executable actions.
    pub fn resolve_debug(&mut self, plan: &actions::ProcedurePlan, frame: &FinalFrameLayout) {
        self.debug.extend(super::debug::resolve_plan(plan, frame));
    }
}

/// Non-executable markers tying the expanded body to selected and allocated program points.
/// They guide verification but cannot assert that a result has a trusted semantic identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Boundary {
    /// Begin the instructions implementing one selected unit.
    UnitStart(SelectedStepId),
    /// End the unit so its declared stack and scratch effects can be checked.
    UnitEnd(SelectedStepId),
    /// Enter a selected segment and its allocation contract.
    Segment(SegmentId),
    /// Apply a selected transfer and the edge's physical stack transition.
    Edge(EdgeId),
    /// Identify generated loop-control flag construction.
    Control(ControlId),
}
/// Executable instructions and structured bodies interleaved with verification metadata.
/// Nested bodies retain their own operations; consumers must recurse to inspect all instructions.
#[derive(Debug, Clone)]
pub(crate) enum AnnotatedOp {
    /// A positional anchor retained across optimization.
    Anchor(Anchor),
    /// One real target instruction, attributed to a selected unit or allocation glue.
    Instruction {
        instruction: masm::Instruction,
        /// Allocation/control glue uses `SelectedStepId(u32::MAX)` rather than a semantic unit.
        unit: SelectedStepId,
    },
    /// A semantic program point with no VM instruction of its own.
    Boundary(Boundary),
    If {
        id: ControlId,
        then_body: AnnotatedMasmBody,
        else_body: AnnotatedMasmBody,
    },
    While {
        id: ControlId,
        body: AnnotatedMasmBody,
    },
    Repeat {
        id: ControlId,
        count: u32,
        body: AnnotatedMasmBody,
    },
}

/// An unverified executable body passed from expansion through optimization to verification.
///
/// Mutation is permitted here so transformations can preserve anchors and unit boundaries.
/// Neither its instruction attribution nor its markers certify semantic result identities;
/// verification checks actual instructions against independently reconstructed source evidence.
#[derive(Debug, Clone, Default)]
pub(crate) struct AnnotatedMasmBody {
    ops: Vec<AnnotatedOp>,
}
impl AnnotatedMasmBody {
    pub fn new() -> Self {
        Self::default()
    }

    /// Edit operations before verification, preserving metadata required by the plan contracts.
    pub fn ops_mut(&mut self) -> &mut Vec<AnnotatedOp> {
        &mut self.ops
    }

    pub fn ops(&self) -> &[AnnotatedOp] {
        &self.ops
    }

    pub fn push(&mut self, op: AnnotatedOp) {
        self.ops.push(op);
    }

    /// Append an instruction with provenance; attribution alone does not authorize its effects.
    pub fn push_instruction(&mut self, instruction: masm::Instruction, unit: SelectedStepId) {
        self.ops.push(AnnotatedOp::Instruction { instruction, unit });
    }

    /// Iterate only instructions in this body’s top-level operation slice.
    ///
    /// Instructions inside `If`, `While`, and `Repeat` bodies are excluded. Consumers that
    /// inspect executable control flow must traverse `ops()` and its nested bodies.
    pub fn top_level_instructions(&self) -> impl Iterator<Item = &masm::Instruction> {
        self.ops.iter().filter_map(|op| match op {
            AnnotatedOp::Instruction { instruction, .. } => Some(instruction),
            _ => None,
        })
    }
}

/// Compiler-internal decorators have no VM stack effect; their position is observable metadata.
#[derive(Debug, Clone)]
pub(crate) enum DebugDecorator {
    Variable(masm::DebugVarInfo),
    InlineCall(masm::DebugInlineCallInfo),
    InlineCallClear,
}
impl DebugDecorator {
    pub fn instruction(&self) -> masm::Instruction {
        match self {
            Self::Variable(info) => masm::Instruction::DebugVar(info.clone()),
            Self::InlineCall(info) => masm::Instruction::DebugInlineCall(info.clone()),
            Self::InlineCallClear => masm::Instruction::DebugInlineCallClear,
        }
    }
}
/// An observable position in an annotated body which optimization must retain.
#[derive(Debug, Clone)]
pub(crate) enum Anchor {
    /// Preserve a program point without emitting a debug decorator.
    Nop,
    /// Bind a plan debug-location identity to its concrete decorator at this position.
    Debug {
        id: DebugLocationId,
        decorator: DebugDecorator,
    },
}

/// Annotate the raw construction's actual structured bodies with local control identities.
/// Identities are assigned in traversal order within this unit, starting at zero. Raw definitions
/// must use resolved repeat counts and explicit `While` bodies; `DoWhile` is unsupported.
pub(crate) fn annotate_raw(ops: &[masm::Op], unit: SelectedStepId) -> AnnotatedMasmBody {
    fn walk(ops: &[masm::Op], unit: SelectedStepId, next: &mut u32) -> AnnotatedMasmBody {
        let mut out = AnnotatedMasmBody::new();
        for op in ops {
            match op {
                masm::Op::Inst(inst) => out.push_instruction(inst.inner().clone(), unit),
                masm::Op::If {
                    then_blk, else_blk, ..
                } => {
                    let id = ControlId(*next);
                    *next += 1;
                    out.push(AnnotatedOp::If {
                        id,
                        then_body: walk(&then_blk.iter().cloned().collect::<Vec<_>>(), unit, next),
                        else_body: walk(&else_blk.iter().cloned().collect::<Vec<_>>(), unit, next),
                    });
                }
                masm::Op::While { body, .. } => {
                    let id = ControlId(*next);
                    *next += 1;
                    out.push(AnnotatedOp::While {
                        id,
                        body: walk(&body.iter().cloned().collect::<Vec<_>>(), unit, next),
                    });
                }
                masm::Op::Repeat { count, body, .. } => {
                    let id = ControlId(*next);
                    *next += 1;
                    out.push(AnnotatedOp::Repeat {
                        id,
                        count: match count {
                            masm::Immediate::Value(n) => *n.inner(),
                            _ => unreachable!("raw repeat count is resolved"),
                        },
                        body: walk(&body.iter().cloned().collect::<Vec<_>>(), unit, next),
                    });
                }
                masm::Op::DoWhile { .. } => {
                    unreachable!("raw definitions use explicit while contracts")
                }
            }
        }
        out
    }
    walk(ops, unit, &mut 0)
}

/// Expand completed actions and the selected structured skeleton; allocation is already finished.
/// The plan must contain actions for every selected segment and resolved bindings for every unit.
/// Missing definitions, bindings, decorators, or invalid frame references produce errors. The
/// returned body still needs optimization and verification before it can be emitted.
pub(crate) fn expand_procedure(
    plan: &actions::ProcedurePlan,
    frame: &FinalFrameLayout,
    registry: &super::units::LoweringRegistry<'_>,
    env: &ExpansionEnv<'_>,
) -> Result<AnnotatedMasmBody, midenc_hir::Report> {
    fn actions(
        out: &mut AnnotatedMasmBody,
        actions: &[actions::PlanAction],
        plan: &actions::ProcedurePlan,
        frame: &FinalFrameLayout,
        registry: &super::units::LoweringRegistry<'_>,
        env: &ExpansionEnv<'_>,
    ) -> Result<(), midenc_hir::Report> {
        for action in actions {
            match action {
                actions::PlanAction::RunLowering { step } => {
                    let unit = plan
                        .selected
                        .segments
                        .iter()
                        .flat_map(|s| &s.units)
                        .find(|u| u.id == *step)
                        .ok_or_else(|| {
                            PlanError::new(
                                DiagnosticCategory::InvalidSelectedLowering,
                                "unknown lowering action",
                            )
                        })?;
                    let bindings = plan.lowering_bindings.get(step).ok_or_else(|| {
                        PlanError::new(
                            DiagnosticCategory::InvalidSelectedLowering,
                            "missing lowering bindings",
                        )
                    })?;
                    out.push(AnnotatedOp::Boundary(Boundary::UnitStart(*step)));
                    let body = registry.get(unit.kind)?.expand(unit, bindings, frame, env)?;
                    out.ops.extend(body.ops);
                    out.push(AnnotatedOp::Boundary(Boundary::UnitEnd(*step)));
                }
                actions::PlanAction::DebugLocation(id) => {
                    let decorator = env
                        .debug
                        .get(id)
                        .ok_or_else(|| {
                            PlanError::new(
                                DiagnosticCategory::InvalidSelectedLowering,
                                "missing debug decorator binding",
                            )
                        })?
                        .clone();
                    out.push(AnnotatedOp::Anchor(Anchor::Debug { id: *id, decorator }));
                }
                _ => {
                    if let Some(instruction) = expand_allocation_action(action, frame)? {
                        out.push_instruction(instruction, SelectedStepId(u32::MAX));
                    }
                }
            }
        }
        Ok(())
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
        plan: &actions::ProcedurePlan,
        frame: &FinalFrameLayout,
        registry: &super::units::LoweringRegistry<'_>,
        env: &ExpansionEnv<'_>,
        loops: &mut Vec<(ControlId, Vec<EdgeId>)>,
    ) -> Result<AnnotatedMasmBody, midenc_hir::Report> {
        let mut out = AnnotatedMasmBody::new();
        match tree {
            ControlTree::Segment(id) => {
                out.push(AnnotatedOp::Boundary(Boundary::Segment(*id)));
                actions(&mut out, &plan.segment_actions[id], plan, frame, registry, env)?;
            }
            ControlTree::Edge(id) => {
                out.push(AnnotatedOp::Boundary(Boundary::Edge(*id)));
                actions(
                    &mut out,
                    plan.edge_actions.get(id).map_or(&[], Vec::as_slice),
                    plan,
                    frame,
                    registry,
                    env,
                )?;
                if let Some((control, _)) = loops.iter().rev().find(|(_, edges)| edges.contains(id))
                {
                    flag(&mut out, *control, true);
                }
            }
            ControlTree::Sequence(children) => {
                for child in children {
                    out.ops.extend(walk(child, plan, frame, registry, env, loops)?.ops);
                }
            }
            ControlTree::If {
                id,
                on_true,
                then_body,
                on_false,
                else_body,
                ..
            } => {
                let mut a = walk(&ControlTree::Edge(*on_true), plan, frame, registry, env, loops)?;
                a.ops.extend(walk(then_body, plan, frame, registry, env, loops)?.ops);
                let mut b = walk(&ControlTree::Edge(*on_false), plan, frame, registry, env, loops)?;
                b.ops.extend(walk(else_body, plan, frame, registry, env, loops)?.ops);
                out.push(AnnotatedOp::If {
                    id: *id,
                    then_body: a,
                    else_body: b,
                });
            }
            ControlTree::Loop {
                id,
                condition,
                before,
                body,
                backedges,
                on_true,
                on_false,
                ..
            } => {
                flag(&mut out, *id, true);
                let mut loop_body = walk(before, plan, frame, registry, env, loops)?;
                loops.push((*id, backedges.clone()));
                let mut a = walk(&ControlTree::Edge(*on_true), plan, frame, registry, env, loops)?;
                a.ops.extend(walk(body, plan, frame, registry, env, loops)?.ops);
                loops.pop();
                let mut b = walk(&ControlTree::Edge(*on_false), plan, frame, registry, env, loops)?;
                flag(&mut b, *id, false);
                loop_body.push(AnnotatedOp::If {
                    id: *condition,
                    then_body: a,
                    else_body: b,
                });
                out.push(AnnotatedOp::While {
                    id: *id,
                    body: loop_body,
                });
            }
        }
        Ok(out)
    }
    walk(&plan.selected.structure, plan, frame, registry, env, &mut Vec::new())
}

/// Finalize a completed physical plan through optimization and mandatory verification.
/// Only a successfully checked body becomes `VerifiedProcedure`; expansion or verification
/// failures propagate without producing an emit-ready procedure.
pub(crate) fn finalize(
    plan: &actions::ProcedurePlan,
    frame: &FinalFrameLayout,
    registry: &super::units::LoweringRegistry<'_>,
    env: &ExpansionEnv<'_>,
) -> Result<crate::verify::VerifiedProcedure, midenc_hir::Report> {
    let mut body = expand_procedure(plan, frame, registry, env)?;
    crate::opt::peephole::simplify_annotated(&mut body);
    Ok(crate::verify::verify(plan, frame, body)?)
}
