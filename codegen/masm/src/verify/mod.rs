//! Source correspondence and independent replay of physical plans, before assembly.
//!
//! Selection checking seals semantic evidence reconstructed from a registered source. Physical
//! verification then replays expanded instructions with independent tokens, liveness, and scratch
//! demand, discharging entry hypotheses on actual transfers. Only the resulting sealed procedure
//! can be converted to MASM without further executable edits.
mod abi;
mod control;
mod dataflow;
mod graph;
mod liveness;
mod memory;
mod reconstruction;
mod replay;
mod results;
mod selection;
mod stack;
mod storage;
mod tokens;

pub(crate) use selection::*;

use crate::{
    lower::expand::{Anchor, AnnotatedMasmBody, AnnotatedOp},
    plan::{actions::ProcedurePlan, *},
};

/// Planning diagnostic enriched by the proof obligation that failed.
pub(crate) type VerificationError = PlanError;

/// Build a verification failure in the relevant planning diagnostic category.
pub(super) fn fail(category: DiagnosticCategory, message: &'static str) -> VerificationError {
    PlanError::new(category, message)
}

/// Report malformed selected or expanded structure without inventing physical-state context.
pub(super) fn error(message: &'static str) -> VerificationError {
    fail(DiagnosticCategory::InvalidSelectedLowering, message)
}

/// Resolve semantic tuple positions without narrowing or accepting a missing lane.
fn result_index(index: u32, length: usize) -> Result<usize, VerificationError> {
    usize::try_from(index)
        .ok()
        .filter(|index| *index < length)
        .ok_or_else(|| error("result relation index outside tuple"))
}

/// Immutable verified instructions. Conversion consumes this value; callers cannot append code.
#[derive(Debug)]
pub(crate) struct VerifiedProcedure {
    body: AnnotatedMasmBody,
    frame: FinalFrameLayout,
    locations: alloc::collections::BTreeMap<SelectedStepId, SourceLocation>,
}

/// Consume an expanded body and prove its control, ABI, storage, and value behavior against
/// the source-checked plan and assigned frame. Failure returns no serializable proof wrapper.
pub(crate) fn verify(
    plan: &ProcedurePlan,
    frame: &FinalFrameLayout,
    body: AnnotatedMasmBody,
) -> Result<VerifiedProcedure, VerificationError> {
    check_anchors(&body)?;
    replay::Verifier::new(plan, frame)?.verify(&body)?;
    Ok(VerifiedProcedure {
        body,
        frame: frame.clone(),
        locations: plan
            .selected
            .segments
            .iter()
            .flat_map(|s| &s.units)
            .map(|u| (u.id, plan.selected.source_map[&u.source].location.clone()))
            .collect(),
    })
}

#[cfg(test)]
pub(crate) mod tests;

/// Independently check the serialized positions, without consulting optimizer decisions.
/// A proof boundary emits nothing. Canonical nops terminate, but cannot divide, variable groups;
/// real instructions (including attributed nops) and inline decorators are grouping barriers.
fn check_anchors(body: &AnnotatedMasmBody) -> Result<(), VerificationError> {
    use crate::lower::expand::DebugDecorator;
    let mut pending_variables = false;
    let mut after_group_anchor = false;
    let mut has_variables = false;
    let mut executable = false;
    let mut empty_nops = 0;
    for op in body.ops() {
        match op {
            AnnotatedOp::Boundary(_) => continue,
            AnnotatedOp::Anchor(Anchor::Debug {
                decorator: DebugDecorator::Variable(_),
                ..
            }) => {
                if after_group_anchor {
                    return Err(error("canonical nop anchor splits a variable-decorator group"));
                }
                pending_variables = true;
                has_variables = true;
                continue;
            }
            AnnotatedOp::Anchor(Anchor::Nop) if pending_variables => {
                pending_variables = false;
                after_group_anchor = true;
                continue;
            }
            _ if pending_variables => {
                return Err(error(
                    "variable-decorator group requires a following canonical nop anchor",
                ));
            }
            AnnotatedOp::Anchor(Anchor::Nop) => empty_nops += 1,
            AnnotatedOp::If {
                then_body,
                else_body,
                ..
            } => {
                executable = true;
                check_anchors(then_body)?;
                check_anchors(else_body)?;
            }
            AnnotatedOp::While { body, .. } | AnnotatedOp::Repeat { body, .. } => {
                executable = true;
                check_anchors(body)?;
            }
            AnnotatedOp::Instruction { .. } => executable = true,
            _ => {}
        }
        after_group_anchor = false;
    }
    if pending_variables {
        return Err(error("variable-decorator group requires a following canonical nop anchor"));
    }
    if empty_nops > 1 || empty_nops != 0 && (executable || has_variables) {
        return Err(error(
            "canonical nop anchor requires a variable group or otherwise empty executable block",
        ));
    }
    Ok(())
}

impl VerifiedProcedure {
    /// Consume the sealed body and serialize its verified instructions and anchors as MASM.
    /// Reject abstract empty blocks lacking an executable nop anchor; conversion itself inserts
    /// no executable operations or storage. The assigned frame determines the local count.
    #[cfg(test)]
    pub fn into_masm(
        self,
        name: crate::masm::ProcedureName,
    ) -> Result<crate::masm::Procedure, VerificationError> {
        self.into_masm_with_span(name, crate::masm::SourceSpan::UNKNOWN)
    }

    /// Serialize a verified procedure with its original source declaration span.
    pub fn into_masm_with_span(
        self,
        name: crate::masm::ProcedureName,
        span: crate::masm::SourceSpan,
    ) -> Result<crate::masm::Procedure, VerificationError> {
        fn serializable(body: &AnnotatedMasmBody) -> Result<(), VerificationError> {
            let mut executable = false;
            for op in body.ops() {
                match op {
                    AnnotatedOp::Instruction { .. } | AnnotatedOp::Anchor(Anchor::Nop) => {
                        executable = true
                    }
                    AnnotatedOp::If {
                        then_body,
                        else_body,
                        ..
                    } => {
                        executable = true;
                        serializable(then_body)?;
                        serializable(else_body)?;
                    }
                    AnnotatedOp::While { body, .. } | AnnotatedOp::Repeat { body, .. } => {
                        executable = true;
                        serializable(body)?;
                    }
                    _ => {}
                }
            }
            if !executable {
                return Err(error(
                    "abstract empty physical proof has no verified executable nop anchor",
                ));
            }
            Ok(())
        }
        serializable(&self.body)?;
        use crate::masm;
        fn convert(
            body: AnnotatedMasmBody,
            locations: &alloc::collections::BTreeMap<SelectedStepId, SourceLocation>,
        ) -> masm::Block {
            let span = masm::SourceSpan::UNKNOWN;
            let ops = body
                .ops()
                .iter()
                .filter_map(|op| {
                    Some(match op {
                        AnnotatedOp::Boundary(_) => return None,
                        AnnotatedOp::Anchor(Anchor::Nop) => {
                            masm::Op::Inst(masm::Span::unknown(masm::Instruction::Nop))
                        }
                        AnnotatedOp::Anchor(Anchor::Debug { decorator, .. }) => {
                            masm::Op::Inst(masm::Span::unknown(decorator.instruction()))
                        }
                        AnnotatedOp::Instruction { instruction, unit } => {
                            masm::Op::Inst(masm::Span::new(
                                locations.get(unit).map_or(span, |l| l.span),
                                instruction.clone(),
                            ))
                        }
                        AnnotatedOp::If {
                            then_body,
                            else_body,
                            ..
                        } => masm::Op::If {
                            span,
                            then_blk: convert(then_body.clone(), locations),
                            else_blk: convert(else_body.clone(), locations),
                        },
                        AnnotatedOp::While { body, .. } => masm::Op::While {
                            span,
                            body: convert(body.clone(), locations),
                        },
                        AnnotatedOp::Repeat { body, count, .. } => masm::Op::Repeat {
                            span,
                            count: (*count).into(),
                            body: convert(body.clone(), locations),
                        },
                    })
                })
                .collect();
            masm::Block::new(span, ops)
        }
        let body = convert(self.body, &self.locations);
        let mut invoked = alloc::collections::BTreeSet::new();
        fn calls(body: &masm::Block, out: &mut alloc::collections::BTreeSet<masm::Invoke>) {
            for op in body.iter() {
                match op {
                    masm::Op::Inst(inst) => {
                        let (kind, target) = match inst.inner() {
                            masm::Instruction::ProcRef(t) => (masm::InvokeKind::ProcRef, t),
                            masm::Instruction::Exec(t) => (masm::InvokeKind::Exec, t),
                            masm::Instruction::Call(t) => (masm::InvokeKind::Call, t),
                            masm::Instruction::SysCall(t) => (masm::InvokeKind::SysCall, t),
                            _ => continue,
                        };
                        out.insert(masm::Invoke::new(kind, target.clone()));
                    }
                    masm::Op::If {
                        then_blk, else_blk, ..
                    } => {
                        calls(then_blk, out);
                        calls(else_blk, out);
                    }
                    masm::Op::While { body, .. } | masm::Op::Repeat { body, .. } => {
                        calls(body, out)
                    }
                    masm::Op::DoWhile { .. } => unreachable!(),
                }
            }
        }
        calls(&body, &mut invoked);
        let mut procedure = masm::Procedure::new(
            span,
            masm::Visibility::Private,
            name,
            u16::try_from(self.frame.aligned_size()).expect("checked final frame"),
            body,
        );
        procedure.extend_invoked(invoked);
        Ok(procedure)
    }
}
