//! Expansion receives only already assigned storage; no allocation state or emitter is available.
use alloc::vec::Vec;

use crate::{masm, plan::*};

#[derive(Default)]
pub(crate) struct ExpansionEnv<'a> {
    /// Linked information available to definitions which require it.
    pub link_info: Option<&'a crate::linker::LinkInfo>,
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
    /// One real target instruction, attributed to a selected unit or allocation glue.
    Instruction {
        instruction: masm::Instruction,
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

/// An unverified executable body. Boundaries cannot publish trusted semantic result identities.
#[derive(Debug, Clone, Default)]
pub(crate) struct AnnotatedMasmBody {
    ops: Vec<AnnotatedOp>,
}

impl AnnotatedMasmBody {
    pub fn new() -> Self {
        Self::default()
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
