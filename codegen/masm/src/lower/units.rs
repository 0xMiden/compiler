//! Registered executable definitions for selected lowering units.
//!
//! Selection declares stack inputs, stored inputs, outputs, and scratch needs before allocation.
//! Definitions expand those declarations using bindings from the completed frame, without
//! allocating storage. Their source-derived evidence is checked independently against actual
//! expanded instructions; a registry entry alone does not certify an allocation candidate.
use alloc::collections::BTreeMap;

use midenc_hir::Report;

use super::expand::{AnnotatedMasmBody, ExpansionEnv};
use crate::{masm, plan::*};

/// Physical input order and locations assigned to one unit.
/// The keys must match the selected declaration exactly; stored contents are checked separately.
#[derive(Debug, Clone, Default)]
pub(crate) struct LoweringBindings {
    /// Selected whole-operand order, checked against source permission before expansion/replay.
    pub input_order: InputOrder,
    /// Spill slots holding inputs which the lowering loads rather than receives on the stack.
    pub stored_inputs: BTreeMap<StoredInputId, SpillSlotId>,
    /// Final allocations of declared scratch objects, including layout and lifetime.
    pub scratch: BTreeMap<ScratchId, ScratchAllocation>,
}

impl LoweringBindings {
    /// Resolve declared scratch from the already assigned frame, without allocating storage.
    pub fn resolve_scratch(
        &self,
        unit: &SelectedLowering,
        frame: &FinalFrameLayout,
    ) -> Result<Self, PlanError> {
        let scratch = unit
            .scratch
            .iter()
            .map(|object| {
                frame
                    .scratch()
                    .get(&object.id)
                    .copied()
                    .map(|allocation| (object.id, allocation))
                    .ok_or_else(|| {
                        PlanError::new(
                            DiagnosticCategory::InvalidSelectedLowering,
                            "unassigned lowering scratch",
                        )
                    })
            })
            .collect::<Result<_, _>>()?;
        let resolved = Self {
            input_order: self.input_order,
            stored_inputs: self.stored_inputs.clone(),
            scratch,
        };
        resolved.validate(unit, frame)?;
        Ok(resolved)
    }

    /// Validate the exact binding domain. Token contents are independently checked by verification.
    pub fn validate(
        &self,
        unit: &SelectedLowering,
        frame: &FinalFrameLayout,
    ) -> Result<(), PlanError> {
        if self.stored_inputs.len() != unit.stored_inputs.len()
            || self.scratch.len() != unit.scratch.len()
        {
            return Err(PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "lowering binding domain does not match declaration",
            ));
        }
        let mut seen = alloc::collections::BTreeSet::new();
        for input in &unit.stored_inputs {
            if !seen.insert(input.id) {
                return Err(PlanError::new(
                    DiagnosticCategory::InvalidSelectedLowering,
                    "duplicate stored-input ID",
                ));
            }
            let slot = *self.stored_inputs.get(&input.id).ok_or_else(|| {
                PlanError::new(
                    DiagnosticCategory::InvalidSpillSlotRead,
                    "missing stored-input binding",
                )
            })?;
            frame.resolve(FrameRef::Spill(slot))?;
        }
        let mut seen = alloc::collections::BTreeSet::new();
        for requirement in &unit.scratch {
            requirement.validate()?;
            if !seen.insert(requirement.id) {
                return Err(PlanError::new(
                    DiagnosticCategory::InvalidSelectedLowering,
                    "duplicate scratch ID",
                ));
            }
            let allocation = self.scratch.get(&requirement.id).ok_or_else(|| {
                PlanError::new(
                    DiagnosticCategory::InvalidSelectedLowering,
                    "missing scratch binding",
                )
            })?;
            if frame.scratch().get(&requirement.id) != Some(allocation)
                || allocation.size != requirement.size
                || allocation.alignment < requirement.alignment
                || allocation.lifetime != requirement.lifetime
            {
                return Err(PlanError::new(
                    DiagnosticCategory::InvalidSelectedLowering,
                    "scratch binding disagrees with declared or assigned layout",
                ));
            }
        }
        Ok(())
    }
}

/// A closed instruction construction registered under a selected lowering kind.
///
/// Implementations consume source-fixed semantics and already assigned storage. They must emit
/// only their declared behavior, preserve unrelated stack lanes, and never allocate or schedule.
/// Expansion returns unverified code; source evidence and physical verification establish trust.
pub(crate) trait LoweringDefinition {
    /// Identity used to match a selected unit with its registered construction.
    fn kind(&self) -> LoweringKind;
    /// Materialize the unit using its final bindings and frame.
    /// Invalid bindings, unsupported source parameters, and unresolved frame references are errors.
    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        env: &ExpansionEnv,
    ) -> Result<AnnotatedMasmBody, Report>;
}

/// Definitions available during expansion of a selected program.
/// Each lowering kind has exactly one definition; the registry does not infer missing definitions.
#[derive(Default)]
pub(crate) struct LoweringRegistry<'a> {
    definitions: BTreeMap<LoweringKind, &'a dyn LoweringDefinition>,
}

impl<'a> LoweringRegistry<'a> {
    /// Register a definition, rejecting a second construction for the same lowering kind.
    pub fn register(&mut self, definition: &'a dyn LoweringDefinition) -> Result<(), PlanError> {
        if self.definitions.contains_key(&definition.kind()) {
            return Err(PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "duplicate lowering definition",
            ));
        }
        self.definitions.insert(definition.kind(), definition);
        Ok(())
    }

    /// Resolve a selected kind, or report an invalid lowering when it was never registered.
    pub fn get(&self, kind: LoweringKind) -> Result<&'a dyn LoweringDefinition, PlanError> {
        self.definitions.get(&kind).copied().ok_or_else(|| {
            PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "unregistered lowering definition",
            )
        })
    }
}

pub(crate) mod arithmetic;
pub(crate) mod calls;
pub(crate) mod fpi;
pub(crate) mod intrinsics;
pub(crate) mod memory;

fn invalid(message: &str) -> PlanError {
    PlanError::new(DiagnosticCategory::InvalidSelectedLowering, message)
}

fn selected(
    kind: LoweringKind,
    id: SelectedStepId,
    source: SourceStepId,
    stack: &[Atom],
    stored: &[StoredInput],
    outputs: &[Atom],
) -> SelectedLowering {
    let semantic_inputs = stack
        .iter()
        .copied()
        .chain(stored.iter().filter_map(|s| match s.value {
            StoredValue::Atom(a) => Some(a),
            _ => None,
        }))
        .map(|a| a.value)
        .collect::<alloc::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    SelectedLowering {
        id,
        kind,
        source,
        semantic_inputs,
        stack_inputs: stack.to_vec(),
        stored_inputs: stored.to_vec(),
        outputs: outputs.to_vec(),
        publication: OutputPublication::Stack,
        address_results: alloc::vec![],
        effects: Default::default(),
        scratch: alloc::vec![],
        scratch_effects: alloc::vec![],
    }
}

/// The small scalar primitives also serve structured generated definitions. Their executable
/// operations are the actual target primitives, with explicit identity publication for aliases.
#[derive(Debug, Clone, Copy)]
#[cfg(test)]
pub(crate) enum Scalar {
    Add,
    Neq,
    Identity,
    Zero,
}
#[cfg(test)]
impl Scalar {
    /// Declare scalar lane arity and source-derived identity evidence.
    /// `Identity` publishes its unchanged input lane; all variants require exactly one result.
    pub fn select(
        self,
        id: SelectedStepId,
        source: SourceStepId,
        inputs: &[Atom],
        outputs: &[Atom],
    ) -> Result<(SelectedLowering, crate::verify::UnitEvidence), PlanError> {
        let arity = match self {
            Self::Add | Self::Neq => 2,
            Self::Identity => 1,
            Self::Zero => 0,
        };
        if inputs.len() != arity || outputs.len() != 1 {
            return Err(invalid("scalar definition arity mismatch"));
        }
        let mut evidence = crate::verify::UnitEvidence::default();
        if matches!(self, Self::Identity) {
            evidence.aliases = LaneAliases::new(alloc::vec![(outputs[0], inputs[0])]);
        }
        Ok((selected(self.kind(), id, source, inputs, &[], outputs), evidence))
    }
}
#[cfg(test)]
impl LoweringDefinition for Scalar {
    fn kind(&self) -> LoweringKind {
        LoweringKind(
            1200 + match self {
                Self::Add => 0,
                Self::Neq => 1,
                Self::Identity => 2,
                Self::Zero => 3,
            },
        )
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        _: &ExpansionEnv,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        let mut body = AnnotatedMasmBody::new();
        let instruction = match self {
            Self::Add => Some(masm::Instruction::Add),
            Self::Neq => Some(masm::Instruction::Neq),
            Self::Identity => None,
            Self::Zero => Some(Primitive::Push(miden_core::Felt::ZERO).instruction()?),
        };
        if let Some(instruction) = instruction {
            body.push_instruction(instruction, unit.id);
        }
        Ok(body)
    }
}

/// An executable definition reconstructed from an actual registered HIR source rule.
/// The candidate selected graph cannot supply this instruction construction.
pub(crate) struct HirUnit {
    /// Per-selection identity matching the unit in the selected program.
    pub kind: LoweringKind,
    /// Closed instruction algorithm with its source parameters already captured.
    pub body: HirBody,
}

/// Source-owned construction families used by final-HIR selection.
/// Dynamic storage offsets are supplied only through the final frame and validated bindings.
pub(crate) enum HirBody {
    /// Typed arithmetic, conversions, assertions, and scalar representation operations.
    Arithmetic(alloc::boxed::Box<arithmetic::Arithmetic>),
    /// Typed memory operations, possibly requiring a resolved source-local address.
    Memory(alloc::boxed::Box<memory::Memory>),
    /// Fixed-stack target primitives and their checked event or intrinsic parameters.
    Intrinsic(alloc::boxed::Box<intrinsics::Intrinsic>),
    /// A source rule's fully fixed straight-line instruction sequence.
    Plain(alloc::vec::Vec<masm::Instruction>),
    /// One phase of a checked indirect call, with explicit storage between phases.
    Indirect(calls::Indirect),
    /// A resolved direct invocation with its declared procedure ABI.
    Direct(calls::Direct),
    /// Stored protocol prefix plus stack payload.
    Fpi,
}

impl LoweringDefinition for HirUnit {
    fn kind(&self) -> LoweringKind {
        self.kind
    }

    fn expand(
        &self,
        unit: &SelectedLowering,
        bindings: &LoweringBindings,
        frame: &FinalFrameLayout,
        env: &ExpansionEnv,
    ) -> Result<AnnotatedMasmBody, Report> {
        bindings.validate(unit, frame)?;
        match &self.body {
            HirBody::Arithmetic(definition) => Ok(definition.expand(unit)),
            HirBody::Memory(definition) => definition.expand(unit, frame),
            HirBody::Intrinsic(definition) => Ok(definition.expand(unit)),
            HirBody::Plain(instructions) => {
                let mut body = AnnotatedMasmBody::new();
                for instruction in instructions {
                    body.push_instruction(instruction.clone(), unit.id);
                }
                Ok(body)
            }
            HirBody::Direct(definition) => definition.expand(unit, bindings, frame, env),
            HirBody::Indirect(definition) => definition.expand(unit, bindings, frame, env),
            HirBody::Fpi => fpi::Fpi.expand(unit, bindings, frame, env),
        }
    }
}
