//! Registered executable definitions for selected lowering units.
//!
//! Selection declares stack inputs, stored inputs, outputs, and scratch needs before allocation.
//! Definitions expand those declarations using bindings from the completed frame, without
//! allocating storage. Their source-derived evidence is checked independently against actual
//! expanded instructions; a registry entry alone does not certify an allocation candidate.
use alloc::collections::BTreeMap;

use midenc_hir::Report;

use super::expand::{AnnotatedMasmBody, ExpansionEnv};
use crate::plan::*;

/// Physical locations assigned to one unit's non-stack inputs and declared scratch objects.
/// The keys must match the selected declaration exactly; stored contents are checked separately.
#[derive(Debug, Clone, Default)]
pub(crate) struct LoweringBindings {
    /// Spill slots holding inputs which the lowering loads rather than receives on the stack.
    pub stored_inputs: BTreeMap<StoredInputId, SpillSlotId>,
    /// Final allocations of declared scratch objects, including layout and lifetime.
    pub scratch: BTreeMap<ScratchId, ScratchAllocation>,
}
impl LoweringBindings {
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
        env: &ExpansionEnv<'_>,
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
