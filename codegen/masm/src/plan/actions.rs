//! Allocator action streams and the proposed physical implementation of a selected procedure.
use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt;

use miden_core::Felt;

use super::*;
use crate::lower::units::LoweringBindings;

/// Allocator decisions replayed against expanded instructions. Stack depths are zero-based;
/// slot references stay symbolic until final frame assignment. `RunLowering` executes a selected
/// unit using its declared input and storage bindings.
///
/// `StoreLane` pops the top lane into a slot, replacing its previous contents. `LoadLane`
/// pushes a copy of an initialized slot without consuming it. `DebugLocation` marks a
/// nonsemantic source-location anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanAction {
    Dup {
        depth: u8,
    },
    Swap {
        depth: u8,
    },
    /// Exchange the top four lanes with word 1, 2 or 3, preserving limb order.
    /// Both complete words must be owned and within the addressable sixteen-lane window.
    SwapWord {
        word: u8,
    },
    MoveUp {
        depth: u8,
    },
    MoveDown {
        depth: u8,
    },
    Drop,
    PushLiteral(Felt),
    StoreLane(SpillSlotId),
    LoadLane(SpillSlotId),
    RunLowering {
        step: SelectedStepId,
    },
    DebugLocation(DebugLocationId),
}
impl fmt::Display for PlanAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dup { depth } => write!(f, "dup {depth}"),
            Self::Swap { depth } => write!(f, "swap {depth}"),
            Self::SwapWord { word } => write!(f, "swapw {word}"),
            Self::MoveUp { depth } => write!(f, "movup {depth}"),
            Self::MoveDown { depth } => write!(f, "movdn {depth}"),
            Self::Drop => f.write_str("drop"),
            Self::PushLiteral(value) => write!(f, "push {value}"),
            Self::StoreLane(slot) => write!(f, "store {slot}"),
            Self::LoadLane(slot) => write!(f, "load {slot}"),
            Self::RunLowering { step } => write!(f, "run s{}", step.0),
            Self::DebugLocation(id) => write!(f, "debug d{}", id.0),
        }
    }
}
/// Proposed physical implementation of a source-checked selected procedure.
/// Allocation can choose actions and contracts but cannot modify the sealed semantic program;
/// independent verification must discharge those choices before serialization.
#[derive(Debug, Clone)]
pub(crate) struct ProcedurePlan {
    /// Actual preparation and contract choices, retained without allocator-state copies.
    pub decisions: diagnostics::PlanDecisions,
    /// Relevant physical observations recorded without retaining values or requesting cells.
    pub debug_locations: BTreeMap<DebugLocationId, crate::lower::debug::AllocatedObservation>,
    pub selected: crate::verify::CheckedSelectedProgram,
    /// Entry hypotheses that every actual incoming edge must establish.
    pub contracts: BTreeMap<SegmentId, EntryContract>,
    /// Ordered actions for each straight-line selected segment.
    pub segment_actions: BTreeMap<SegmentId, Vec<PlanAction>>,
    /// Physical transfers executed before simultaneous semantic parameter renaming.
    pub edge_actions: BTreeMap<EdgeId, Vec<PlanAction>>,
    /// Complete symbolic slot domain to be assigned by the final frame layout.
    pub spill_slots: Vec<SpillSlotId>,
    /// Sorted unique backing cells needed by semantic inputs at each prepared unit boundary.
    /// These allocation facts extend storage lifetimes without emitting loads or granting
    /// verification authority. Equal resident lanes and literals need no backing read.
    pub semantic_storage_reads: BTreeMap<SelectedStepId, Vec<SpillSlotId>>,
    /// Physical input order and storage choices supplied to expansion for each selected unit.
    pub lowering_bindings: BTreeMap<SelectedStepId, LoweringBindings>,
}
impl fmt::Display for ProcedurePlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "procedure {:?}", self.selected.abi)?;
        for segment in &self.selected.segments {
            writeln!(f, "segment b{}", segment.id.0)?;
            if let Some(decision) = self.decisions.entries.get(&segment.id) {
                write!(f, "  contract={} costs=[", decision.origin)?;
                for (index, cost) in decision.costs.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}:{}", cost.origin, cost.actions)?;
                }
                writeln!(f, "]")?;
            }
            if let Some(contract) = self.contracts.get(&segment.id) {
                write!(f, "  entry [")?;
                for (i, atom) in contract.stack_top_first.iter().enumerate() {
                    if i != 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{atom}")?;
                }
                writeln!(f, "] {:?}", contract.tail)?;
                for (slot, atom) in &contract.required_spill_slots {
                    writeln!(f, "  require {slot}={atom}")?;
                }
            }
            for unit in &segment.units {
                writeln!(f, "  unit s{} kind={} source={}", unit.id.0, unit.kind.0, unit.source.0)?;
                if let Some(reads) = self.semantic_storage_reads.get(&unit.id) {
                    writeln!(f, "    semantic-storage-reads={reads:?}")?;
                }
                if let Some(decision) = self.decisions.preparations.get(&unit.id) {
                    writeln!(f, "    tier={:?} victims={:?}", decision.tier, decision.victims)?;
                }
                if let Some(source) = self.selected.source_map.get(&unit.source) {
                    writeln!(
                        f,
                        "    origin {:?} inline={:?}",
                        source.location.span, source.location.inline_chain
                    )?;
                }
                writeln!(
                    f,
                    "    stack={:?} stored={:?} outputs={:?}",
                    unit.stack_inputs, unit.stored_inputs, unit.outputs
                )?;
            }
            if let Some(actions) = self.segment_actions.get(&segment.id) {
                let control = self.decisions.controls.get(&segment.id);
                let start = control.map_or(actions.len(), |(start, ..)| *start);
                let (loads, stores) = traffic(&actions[..start]);
                writeln!(f, "  operand-traffic loads={loads} stores={stores}")?;
                if let Some((_, edge, decision)) = control {
                    let (loads, stores) = traffic(&actions[start..]);
                    writeln!(
                        f,
                        "  edge-preparation e{} tier={:?} victims={:?} loads={loads} \
                         stores={stores}",
                        edge.0, decision.tier, decision.victims
                    )?;
                }
                for action in actions {
                    writeln!(f, "  {action}")?;
                }
            }
        }
        for (edge, actions) in &self.edge_actions {
            let (loads, stores) = traffic(actions);
            writeln!(f, "edge e{} edge-traffic loads={loads} stores={stores}", edge.0)?;
            for action in actions {
                writeln!(f, "  {action}")?;
            }
        }
        Ok(())
    }
}

fn traffic(actions: &[PlanAction]) -> (usize, usize) {
    (
        actions.iter().filter(|a| matches!(a, PlanAction::LoadLane(_))).count(),
        actions.iter().filter(|a| matches!(a, PlanAction::StoreLane(_))).count(),
    )
}
