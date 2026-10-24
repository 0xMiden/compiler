//! Scheduling for independent backward verifier analyses, without allocator facts.
use alloc::{
    collections::{BTreeMap, VecDeque},
    vec,
    vec::Vec,
};

use crate::plan::SelectedProgram;

/// Indexed graph dependencies and a deduplicated backward work queue.
///
/// Positions refer to the selected segment/edge vectors, never to numeric IDs. Every
/// segment is visited initially, even if its entry demand stays empty. Analyses read
/// current successor facts, refresh all boundary facts on each visit, and notify this
/// queue only when entry demand changes. Transfer semantics remain with each analysis.
pub(super) struct BackwardWorklist {
    outgoing: Vec<Vec<usize>>,
    predecessors: Vec<Vec<usize>>,
    pending: VecDeque<usize>,
    queued: Vec<bool>,
}
impl BackwardWorklist {
    /// Build from an independently checked graph. Preserve parallel edges because
    /// each edge may substitute a different simultaneous argument/content binding.
    pub fn new(program: &SelectedProgram) -> Self {
        let count = program.segments.len();
        let indices: BTreeMap<_, _> = program
            .segments
            .iter()
            .enumerate()
            .map(|(index, segment)| (segment.id, index))
            .collect();
        let mut outgoing = vec![Vec::new(); count];
        let mut predecessors = vec![Vec::new(); count];
        for (index, edge) in program.edges.iter().enumerate() {
            let source = indices[&edge.source];
            outgoing[source].push(index);
            if let Some(destination) = edge.destination {
                predecessors[indices[&destination]].push(source);
            }
        }
        Self {
            outgoing,
            predecessors,
            pending: (0..count).rev().collect(),
            queued: vec![true; count],
        }
    }

    /// Clear membership before evaluation so a changed self-loop can requeue itself.
    pub fn next(&mut self) -> Option<usize> {
        let index = self.pending.pop_front()?;
        self.queued[index] = false;
        Some(index)
    }

    /// Outgoing edge positions for this segment, in source order.
    pub fn outgoing(&self, index: usize) -> &[usize] {
        &self.outgoing[index]
    }

    /// Only predecessors depend on this entry fact; pending work already observes
    /// the latest value and needs no duplicate queue entry.
    pub fn changed(&mut self, index: usize) {
        for &predecessor in &self.predecessors[index] {
            if !self.queued[predecessor] {
                self.pending.push_back(predecessor);
                self.queued[predecessor] = true;
            }
        }
    }
}
