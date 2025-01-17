use core::fmt;

use smallvec::SmallVec;

use crate::{adt::SmallMap, BlockRef};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum CfgUpdateKind {
    Insert,
    Delete,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct CfgUpdate {
    kind: CfgUpdateKind,
    from: BlockRef,
    to: BlockRef,
}
impl CfgUpdate {
    #[inline(always)]
    pub const fn kind(&self) -> CfgUpdateKind {
        self.kind
    }

    #[inline(always)]
    pub const fn from(&self) -> BlockRef {
        self.from
    }

    #[inline(always)]
    pub const fn to(&self) -> BlockRef {
        self.to
    }
}
impl fmt::Debug for CfgUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(match self.kind {
            CfgUpdateKind::Insert => "Insert",
            CfgUpdateKind::Delete => "Delete",
        })
        .field("from", &self.from)
        .field("to", &self.to)
        .finish()
    }
}

#[derive(Default, Clone)]
struct DeletesInserts {
    deletes: SmallVec<[BlockRef; 2]>,
    inserts: SmallVec<[BlockRef; 2]>,
}
impl DeletesInserts {
    pub fn di(&self, is_insert: bool) -> &SmallVec<[BlockRef; 2]> {
        if is_insert {
            &self.inserts
        } else {
            &self.deletes
        }
    }

    pub fn di_mut(&mut self, is_insert: bool) -> &mut SmallVec<[BlockRef; 2]> {
        if is_insert {
            &mut self.inserts
        } else {
            &mut self.deletes
        }
    }
}

pub trait GraphDiff {
    fn is_empty(&self) -> bool;
    fn legalized_updates(&self) -> &[CfgUpdate];
    fn num_legalized_updates(&self) -> usize {
        self.legalized_updates().len()
    }
    fn pop_update_for_incremental_updates(&mut self) -> CfgUpdate;
    fn get_children<const INVERSE_EDGE: bool>(&self, node: BlockRef) -> SmallVec<[BlockRef; 8]>;
}

/// GraphDiff defines a CFG snapshot: given a set of Update<NodePtr>, provides
/// a getChildren method to get a Node's children based on the additional updates
/// in the snapshot. The current diff treats the CFG as a graph rather than a
/// multigraph. Added edges are pruned to be unique, and deleted edges will
/// remove all existing edges between two blocks.
///
/// Two booleans are used to define orders in graphs:
/// InverseGraph defines when we need to reverse the whole graph and is as such
/// also equivalent to applying updates in reverse.
/// InverseEdge defines whether we want to change the edges direction. E.g., for
/// a non-inversed graph, the children are naturally the successors when
/// InverseEdge is false and the predecessors when InverseEdge is true.
#[derive(Clone)]
pub struct CfgDiff<const INVERSE_GRAPH: bool = false> {
    succ: SmallMap<BlockRef, DeletesInserts>,
    pred: SmallMap<BlockRef, DeletesInserts>,
    /// By default, it is assumed that, given a CFG and a set of updates, we wish
    /// to apply these updates as given. If UpdatedAreReverseApplied is set, the
    /// updates will be applied in reverse: deleted edges are considered re-added
    /// and inserted edges are considered deleted when returning children.
    updated_are_reverse_applied: bool,
    /// Keep the list of legalized updates for a deterministic order of updates
    /// when using a GraphDiff for incremental updates in the DominatorTree.
    /// The list is kept in reverse to allow popping from end.
    legalized_updates: SmallVec<[CfgUpdate; 4]>,
}

impl<const INVERSE_GRAPH: bool> Default for CfgDiff<INVERSE_GRAPH> {
    fn default() -> Self {
        Self {
            succ: Default::default(),
            pred: Default::default(),
            updated_are_reverse_applied: false,
            legalized_updates: Default::default(),
        }
    }
}

impl<const INVERSE_GRAPH: bool> CfgDiff<INVERSE_GRAPH> {
    pub fn new<I>(updates: I, reverse_apply_updates: bool) -> Self
    where
        I: ExactSizeIterator<Item = CfgUpdate>,
    {
        let mut this = Self {
            legalized_updates: legalize_updates(updates, INVERSE_GRAPH, false),
            ..Default::default()
        };
        for update in this.legalized_updates.iter() {
            let is_insert = matches!(update.kind(), CfgUpdateKind::Insert) || reverse_apply_updates;
            this.succ.entry(update.from).or_default().di_mut(is_insert).push(update.to);
            this.pred.entry(update.to).or_default().di_mut(is_insert).push(update.from);
        }
        this.updated_are_reverse_applied = reverse_apply_updates;
        this
    }
}

impl<const INVERSE_GRAPH: bool> GraphDiff for CfgDiff<INVERSE_GRAPH> {
    fn is_empty(&self) -> bool {
        self.succ.is_empty() && self.pred.is_empty() && self.legalized_updates.is_empty()
    }

    #[inline(always)]
    fn legalized_updates(&self) -> &[CfgUpdate] {
        &self.legalized_updates
    }

    fn pop_update_for_incremental_updates(&mut self) -> CfgUpdate {
        assert!(!self.legalized_updates.is_empty(), "no updates to apply");
        let update = self.legalized_updates.pop().unwrap();
        let is_insert =
            matches!(update.kind(), CfgUpdateKind::Insert) || self.updated_are_reverse_applied;
        let succ_di_list = &mut self.succ[&update.from];
        let is_empty = {
            let succ_list = succ_di_list.di_mut(is_insert);
            assert_eq!(succ_list.last(), Some(&update.to));
            succ_list.pop();
            succ_list.is_empty()
        };
        if is_empty && succ_di_list.di(!is_insert).is_empty() {
            self.succ.remove(&update.from);
        }

        let pred_di_list = &mut self.pred[&update.to];
        let pred_list = pred_di_list.di_mut(is_insert);
        assert_eq!(pred_list.last(), Some(&update.from));
        pred_list.pop();
        if pred_list.is_empty() && pred_di_list.di(!is_insert).is_empty() {
            self.pred.remove(&update.to);
        }
        update
    }

    fn get_children<const INVERSE_EDGE: bool>(&self, node: BlockRef) -> SmallVec<[BlockRef; 8]> {
        let mut r = crate::dominance::nca::get_children::<INVERSE_EDGE>(node);
        if !INVERSE_EDGE {
            r.reverse();
        }

        let children = if INVERSE_EDGE != INVERSE_GRAPH {
            &self.pred
        } else {
            &self.succ
        };
        let Some(found) = children.get(&node) else {
            return r;
        };

        // Remove children present in the CFG but not in the snapshot.
        for child in found.di(false) {
            r.retain(|c| c != child);
        }

        // Add children present in the snapshot for not in the real CFG.
        r.extend(found.di(true).iter().cloned());

        r
    }
}

/// `legalize_updates` simplifies updates assuming a graph structure.
///
/// This function serves double purpose:
///
/// 1. It removes redundant updates, which makes it easier to reverse-apply them when traversing
///    CFG.
/// 2. It optimizes away updates that cancel each other out, as the end result is the same.
fn legalize_updates<I>(
    all_updates: I,
    inverse_graph: bool,
    reverse_result_order: bool,
) -> SmallVec<[CfgUpdate; 4]>
where
    I: ExactSizeIterator<Item = CfgUpdate>,
{
    #[derive(Default, Copy, Clone)]
    struct UpdateOp {
        num_insertions: i32,
        index: u32,
    }

    // Count the total number of inserions of each edge.
    // Each insertion adds 1 and deletion subtracts 1. The end number should be one of:
    //
    // * `-1` (deletion)
    // * `0` (NOP),
    // * `1` (insertion).
    //
    // Otherwise, the sequence of updates contains multiple updates of the same kind and we assert
    // for that case.
    let mut operations =
        SmallMap::<(BlockRef, BlockRef), UpdateOp, 4>::with_capacity(all_updates.len());

    for (
        i,
        CfgUpdate {
            kind,
            mut from,
            mut to,
        },
    ) in all_updates.enumerate()
    {
        if inverse_graph {
            // Reverse edge for post-dominators
            core::mem::swap(&mut from, &mut to);
        }

        operations
            .entry((from, to))
            .or_insert_with(|| UpdateOp {
                num_insertions: 0,
                index: i as u32,
            })
            .num_insertions += match kind {
            CfgUpdateKind::Insert => 1,
            CfgUpdateKind::Delete => -1,
        };
    }

    let mut result = SmallVec::<[CfgUpdate; 4]>::with_capacity(operations.len());
    for (&(from, to), update_op) in operations.iter() {
        assert!(update_op.num_insertions.abs() <= 1, "unbalanced operations!");
        if update_op.num_insertions == 0 {
            continue;
        }
        let kind = if update_op.num_insertions > 0 {
            CfgUpdateKind::Insert
        } else {
            CfgUpdateKind::Delete
        };
        result.push(CfgUpdate { kind, from, to });
    }

    // Make the order consistent by not relying on pointer values within the set. Reuse the old
    // operations map.
    //
    // In the future, we should sort by something else to minimize the amount of work needed to
    // perform the series of updates.
    result.sort_by(|a, b| {
        let op_a = &operations[&(a.from, a.to)];
        let op_b = &operations[&(b.from, b.to)];
        if reverse_result_order {
            op_a.index.cmp(&op_b.index)
        } else {
            op_a.index.cmp(&op_b.index).reverse()
        }
    });

    result
}
