//! Resolve observational variable descriptions without allocating or retaining their operands.
//!
//! Source checking seals observation positions; allocation records only current relevant lane
//! locations. Symbolic cells become signed FMP displacements after the final frame is assigned.
use alloc::{collections::BTreeMap, vec::Vec};

use midenc_hir::{
    Operation,
    dialects::debuginfo::{self, attributes::Expression},
};

use crate::{lower::expand::DebugDecorator, masm, plan::*};

/// A source-owned debug marker before the indexed unit, or at a segment's end.
/// Its value lanes are observations and must never be added to semantic use sets.
#[derive(Debug, Clone)]
pub(crate) struct Observation {
    pub segment: SegmentId,
    pub position: usize,
    pub description: Description,
}

/// Closed inert descriptions carried by source observations.
/// Variable records may query allocation; inline call decorators have no value operand.
#[derive(Debug, Clone)]
pub(crate) enum Description {
    Variable {
        variable: masm::DebugVarInfo,
        expression: Option<Expression>,
        lanes: Vec<Atom>,
        globals: BTreeMap<midenc_hir::interner::Symbol, u32>,
    },
    Inline(DebugDecorator),
}

impl Observation {
    /// Borrow exactly the semantic lanes described by this observational marker.
    pub fn lanes(&self) -> &[Atom] {
        match &self.description {
            Description::Variable { lanes, .. } => lanes,
            Description::Inline(_) => &[],
        }
    }

    /// Emit the same explicit inline-chain reset and frames as the existing source emitter.
    /// Unknown call-site lines stay absent rather than acquiring fabricated source locations.
    pub fn inline_chain(operation: &Operation, segment: SegmentId, position: usize) -> Vec<Self> {
        use miden_assembly::debuginfo::{ColumnNumber, FileLineCol, LineNumber, Uri};
        use midenc_hir::dialects::debuginfo::attributes::{
            INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChainAttr,
        };
        let marker = |decorator| Self {
            segment,
            position,
            description: Description::Inline(decorator),
        };
        let mut observations = alloc::vec![marker(DebugDecorator::InlineCallClear)];
        if let Some(attr) = operation
            .get_attribute(INLINE_CALL_CHAIN_ATTR_NAME)
            .and_then(|attr| attr.try_downcast_attr::<InlineCallChainAttr>().ok())
        {
            for frame in &attr.borrow().frames {
                let Some(line) = LineNumber::new(frame.call_line) else {
                    continue;
                };
                let declaration = FileLineCol::new(
                    Uri::new(frame.file.as_str()),
                    LineNumber::new(frame.line).unwrap_or_default(),
                    ColumnNumber::new(frame.column).unwrap_or_default(),
                );
                let call = FileLineCol::new(
                    Uri::new(frame.call_file.as_str()),
                    line,
                    ColumnNumber::new(frame.call_column).unwrap_or_default(),
                );
                let mut info =
                    masm::DebugInlineCallInfo::new(frame.name.as_str(), declaration, call);
                if let Some(name) = frame.linkage_name {
                    info = info.with_linkage_name(name.as_str());
                }
                observations.push(marker(DebugDecorator::InlineCall(info)));
            }
        }
        observations
    }

    /// Capture variable metadata and expression from an actual registered debug operation.
    /// The caller resolves SSA identities independently of scheduling and supplies no liveness.
    pub fn from_source(
        operation: &Operation,
        segment: SegmentId,
        position: usize,
        lanes: Vec<Atom>,
        env: &crate::select::SelectEnv<'_>,
    ) -> Self {
        let (variable, expression) = if let Some(op) =
            operation.downcast_ref::<debuginfo::DebugValue>()
        {
            (op.variable().as_value().clone(), Some(op.expression().as_value().clone()))
        } else if let Some(op) = operation.downcast_ref::<debuginfo::DebugDeclare>() {
            (op.variable().as_value().clone(), Some(op.expression().as_value().clone()))
        } else {
            let op =
                operation.downcast_ref::<debuginfo::DebugKill>().expect("registered debug rule");
            (op.variable().as_value().clone(), None)
        };
        let mut info =
            masm::DebugVarInfo::new(variable.name.as_str(), masm::DebugVarLocation::Unavailable);
        super::lowering::apply_debug_var_metadata(
            &mut info,
            &variable,
            operation.context().session(),
        );
        let mut globals = BTreeMap::new();
        if let (Some(expression), Some(link)) = (&expression, env.link_info) {
            use midenc_hir::dialects::debuginfo::attributes::{ExpressionOp, FrameBase};
            for op in &expression.operations {
                let symbol = match op {
                    ExpressionOp::GlobalSlot(name)
                    | ExpressionOp::FrameBase {
                        base: FrameBase::GlobalSlot(name),
                        ..
                    } => Some(*name),
                    _ => None,
                };
                if let Some(name) = symbol
                    && let Some(address) =
                        super::lowering::resolve_global_slot_address(name, operation, link)
                {
                    globals.insert(name, address);
                }
            }
        }
        Self {
            segment,
            position,
            description: Description::Variable {
                variable: info,
                expression,
                lanes,
                globals,
            },
        }
    }
}

/// One currently valid lane location: a physical location, or the literal the lane reconstructs
/// to. Cells remain symbolic until frame assignment.
/// Unavailable includes missing values and physical lanes buried beyond the VM's accessible
/// window; a literal lane has no depth, so it is never unavailable for that reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Location {
    /// Zero-based raw depth, with lane zero nearest the top. Only depths0..15 are accessible.
    Stack(u8),
    /// A currently valid symbolic cell, resolved after coloring to an FMP-relative displacement.
    /// All lanes must occupy increasing contiguous cells because the target describes one base.
    Cell(SpillSlotId),
    /// A literal the lane reconstructs to when no physical copy is resident.
    Literal(miden_core::Felt),
    /// No current accessible representation; this never requests a debug-only copy.
    Unavailable,
}

/// The minimum allocation snapshot required to render one source observation.
/// It owns neither the allocation state nor storage requests and cannot influence scheduling.
#[derive(Debug, Clone)]
pub(crate) struct AllocatedObservation {
    pub observation: Observation,
    pub locations: Vec<Location>,
}

/// Describe only a contiguous ordered stack or frame representation supported by the target,
/// or a one-lane literal. Split values, multi-lane literals and out-of-range signed debug
/// displacements are explicitly unavailable.
pub(crate) fn resolve_lanes(
    locations: &[Location],
    frame: &FinalFrameLayout,
) -> masm::DebugVarLocation {
    use masm::DebugVarLocation as D;
    let Some(first) = locations.first() else {
        return D::Unavailable;
    };
    match first {
        Location::Literal(value) if locations.len() == 1 => D::Const(*value),
        Location::Stack(start)
            if locations.iter().enumerate().all(|(i, loc)| {
                usize::from(*start) + i < 16 && *loc == Location::Stack(*start + i as u8)
            }) =>
        {
            D::Stack(*start)
        }
        Location::Cell(slot) => {
            let Ok(start) = frame.resolve(FrameRef::Spill(*slot)) else {
                return D::Unavailable;
            };
            for (i, loc) in locations.iter().enumerate() {
                let Location::Cell(slot) = loc else {
                    return D::Unavailable;
                };
                let Ok(offset) = frame.resolve(FrameRef::Spill(*slot)) else {
                    return D::Unavailable;
                };
                if usize::from(offset) != usize::from(start) + i
                    || displacement(u32::from(offset), frame).is_none()
                {
                    return D::Unavailable;
                }
            }
            displacement(u32::from(start), frame).map(D::Local).unwrap_or(D::Unavailable)
        }
        _ => D::Unavailable,
    }
}

fn displacement(offset: u32, frame: &FinalFrameLayout) -> Option<i16> {
    i16::try_from(i64::from(offset) - i64::from(frame.aligned_size())).ok()
}

impl AllocatedObservation {
    /// Resolve a source expression using the actual final layout, preserving metadata.
    /// Operand reads may use an existing valid cell; no executable materialization is added.
    pub fn decorator(&self, frame: &FinalFrameLayout) -> DebugDecorator {
        use masm::{DebugLocationExpressionOp as E, DebugVarLocation as D};
        let Description::Variable {
            variable,
            expression,
            globals,
            ..
        } = &self.observation.description
        else {
            let Description::Inline(decorator) = &self.observation.description else {
                unreachable!()
            };
            return decorator.clone();
        };
        let mut variable = variable.clone();
        let location = if let Some(expression) = expression {
            let operand = resolve_lanes(&self.locations, frame);
            let read = match operand {
                D::Stack(position) => Some(E::ReadStack(position)),
                D::Local(offset) => Some(E::ReadLocal(offset)),
                D::Const(value) => Some(E::ConstU64(value.as_canonical_u64())),
                _ => None,
            };
            super::lowering::resolve_debug_var_location_with_operand(
                expression,
                read,
                Some(frame.aligned_size()),
                |index| {
                    frame
                        .source()
                        .offsets
                        .get(index as usize)
                        .and_then(|offset| usize::try_from(*offset).ok())
                },
                |name| globals.get(&name).copied(),
            )
        } else {
            D::Unavailable
        };
        variable.set_value_location(location);
        DebugDecorator::Variable(variable)
    }
}

#[cfg(test)]
mod tests;

/// Resolve only cells whose current final-offset writer still matches the symbolic snapshot.
/// Coloring can reuse a dead cached cell without a semantic load; debug observations must not
/// extend that lifetime. Entry contracts seed branch/loop facts, and real scalar stores invalidate
/// earlier occupants. This tracks storage writers only, never allocation or operand-stack state.
pub(crate) fn resolve_plan(
    plan: &crate::plan::actions::ProcedurePlan,
    frame: &FinalFrameLayout,
) -> BTreeMap<DebugLocationId, DebugDecorator> {
    use crate::plan::actions::PlanAction;
    let mut decorators = BTreeMap::new();
    for (segment, actions) in &plan.segment_actions {
        let mut writers = BTreeMap::new();
        if let Some(contract) = plan.contracts.get(segment) {
            for (slot, _) in &contract.required_spill_slots {
                if let Ok(offset) = frame.resolve(FrameRef::Spill(*slot)) {
                    writers.insert(offset, *slot);
                }
            }
        }
        for action in actions {
            match action {
                PlanAction::StoreLane(slot) => {
                    if let Ok(offset) = frame.resolve(FrameRef::Spill(*slot)) {
                        writers.insert(offset, *slot);
                    }
                }
                PlanAction::DebugLocation(id) => {
                    if let Some(record) = plan.debug_locations.get(id) {
                        let mut record = record.clone();
                        for location in &mut record.locations {
                            if let Location::Cell(slot) = location
                                && frame
                                    .resolve(FrameRef::Spill(*slot))
                                    .ok()
                                    .and_then(|offset| writers.get(&offset).copied())
                                    != Some(*slot)
                            {
                                *location = Location::Unavailable;
                            }
                        }
                        decorators.insert(*id, record.decorator(frame));
                    }
                }
                _ => {}
            }
        }
    }
    decorators
}
