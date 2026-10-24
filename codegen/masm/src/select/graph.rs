//! Stable construction, with semantic and structural authority retained by source checking.
use alloc::{collections::BTreeMap, vec::Vec};

use crate::plan::*;

/// Collect selected nodes by identity before attaching their executable control tree.
/// Duplicate identities are rejected, but successful construction is not source verification.
pub(crate) struct GraphBuilder {
    origin: ProcedureOrigin,
    abi: ProcedureAbi,
    values: BTreeMap<ValueId, SelectedValue>,
    sources: BTreeMap<SourceStepId, SourceRecord>,
    segments: BTreeMap<SegmentId, SelectedSegment>,
    edges: BTreeMap<EdgeId, TransferEdge>,
}
impl GraphBuilder {
    /// Start an empty graph with the source origin and externally visible procedure ABI.
    pub fn new(origin: ProcedureOrigin, abi: ProcedureAbi) -> Self {
        Self {
            origin,
            abi,
            values: BTreeMap::new(),
            sources: BTreeMap::new(),
            segments: BTreeMap::new(),
            edges: BTreeMap::new(),
        }
    }

    /// Insert a value, rejecting reuse of its identity.
    pub fn add_value(&mut self, value: SelectedValue) -> Result<(), PlanError> {
        insert(&mut self.values, value.id, value)
    }

    /// Attach source provenance under a fresh coverage identity.
    pub fn add_source(&mut self, id: SourceStepId, source: SourceRecord) -> Result<(), PlanError> {
        insert(&mut self.sources, id, source)
    }

    /// Insert a segment while preserving the order of its lowering units.
    pub fn add_segment(&mut self, segment: SelectedSegment) -> Result<(), PlanError> {
        insert(&mut self.segments, segment.id, segment)
    }

    /// Insert a transfer; its bindings and relation to the control tree still require checking.
    pub fn add_edge(&mut self, edge: TransferEdge) -> Result<(), PlanError> {
        insert(&mut self.edges, edge.id, edge)
    }

    /// Canonicalize storage order without changing executable order or omitting any node.
    /// The independent source checker still validates all bindings and control alternatives.
    /// The first executable segment is stored first; a tree without that segment is rejected.
    pub fn finish(mut self, structure: ControlTree) -> Result<SelectedProgram, PlanError> {
        fn entry(tree: &ControlTree) -> Option<SegmentId> {
            match tree {
                ControlTree::Segment(id) | ControlTree::Loop { header: id, .. } => Some(*id),
                ControlTree::Sequence(children) => children.iter().find_map(entry),
                _ => None,
            }
        }
        let entry = entry(&structure)
            .and_then(|id| self.segments.remove(&id))
            .ok_or_else(|| invalid("selected structure lacks its entry segment"))?;
        let mut segments = Vec::with_capacity(self.segments.len() + 1);
        segments.push(entry);
        segments.extend(self.segments.into_values());
        Ok(SelectedProgram {
            origin: self.origin,
            abi: self.abi,
            values: self.values,
            source_map: self.sources,
            segments,
            edges: self.edges.into_values().collect(),
            structure,
        })
    }
}

fn insert<K: Ord, V>(map: &mut BTreeMap<K, V>, key: K, value: V) -> Result<(), PlanError> {
    match map.entry(key) {
        alloc::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        alloc::collections::btree_map::Entry::Occupied(_) => {
            Err(invalid("duplicate selected graph identity"))
        }
    }
}
fn invalid(message: &'static str) -> PlanError {
    PlanError::new(DiagnosticCategory::InvalidSelectedLowering, message)
}
