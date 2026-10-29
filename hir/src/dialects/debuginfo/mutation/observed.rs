//! Current capture ownership, including detached subtrees.
//!
//! List removal clears the IR parent before notifying us. Each nonempty owner bucket therefore
//! remembers its immediate parent, so removal can prune the old outer path without borrowing or
//! walking a container. Internal paths remain indexed for later attachment. This index is distinct
//! from active permissions' ownership snapshots, which must retain movement history.

use super::CaptureOwner;
use crate::{FxHashMap, FxHashSet, OperationRef, SmallVec};

struct Owner {
    parent: Option<CaptureOwner>,
    records: FxHashSet<OperationRef>,
}

#[derive(Default)]
pub(super) struct ObservedCaptures {
    owners: FxHashMap<CaptureOwner, Owner>,
}

impl ObservedCaptures {
    pub(super) fn observe(&mut self, record: OperationRef) {
        let owner = CaptureOwner::Operation(record);
        self.owners.entry(owner).or_insert_with(|| Owner {
            parent: None,
            records: FxHashSet::from_iter([record]),
        });
        self.reparent(owner);
    }

    /// Reconcile only the moved subtree's outer ancestry. Every current ancestor of an observed
    /// capture has a bucket, including the capture itself; empty historical buckets are removed.
    /// Parent links are read without entity borrows, including when called from intrusive hooks.
    pub(super) fn reparent(&mut self, owner: CaptureOwner) -> SmallVec<[OperationRef; 2]> {
        let Some(bucket) = self.owners.get_mut(&owner) else {
            return SmallVec::new();
        };
        let records = bucket.records.iter().copied().collect::<SmallVec<_>>();
        let parent = owner.parent();
        let mut previous = core::mem::replace(&mut bucket.parent, parent);
        if previous == parent {
            return records;
        }
        while let Some(ancestor) = previous {
            let bucket = self.owners.get_mut(&ancestor).expect("indexed capture ancestor");
            previous = bucket.parent;
            for record in &records {
                bucket.records.remove(record);
            }
            if bucket.records.is_empty() {
                self.owners.remove(&ancestor);
            } else if bucket.records.allocation_size()
                > allocation_limit::<OperationRef>(bucket.records.len())
            {
                bucket.records.shrink_to_fit();
            }
        }
        let mut current = parent;
        while let Some(ancestor) = current {
            current = ancestor.parent();
            self.owners
                .entry(ancestor)
                .or_insert_with(|| Owner {
                    parent: current,
                    records: FxHashSet::default(),
                })
                .records
                .extend(records.iter().copied());
        }
        if self.owners.allocation_size()
            > allocation_limit::<(CaptureOwner, Owner)>(self.owners.len())
        {
            self.owners.shrink_to_fit();
        }
        records
    }
}

/// Compact geometrically based on allocated bytes, not capacity(): tombstones can lower reported
/// growth capacity while retaining the allocation and iteration cost. The two-entry floor covers
/// the smallest table's control bytes; the fourfold threshold provides rehashing hysteresis.
fn allocation_limit<T>(len: usize) -> usize {
    len.max(2).saturating_mul(core::mem::size_of::<T>()).saturating_mul(4)
}

#[cfg(test)]
pub(super) fn allocation_sizes(index: &ObservedCaptures) -> (usize, usize, usize) {
    (
        index.owners.allocation_size(),
        index.owners.values().map(|owner| owner.records.allocation_size()).sum(),
        core::mem::size_of::<(CaptureOwner, Owner)>(),
    )
}

#[cfg(test)]
pub(super) fn membership_counts(index: &ObservedCaptures) -> (usize, usize) {
    (index.owners.len(), index.owners.values().map(|owner| owner.records.len()).sum())
}

#[cfg(test)]
pub(super) fn capacities(index: &ObservedCaptures) -> (usize, usize) {
    (
        index.owners.capacity(),
        index.owners.values().map(|owner| owner.records.capacity()).sum(),
    )
}

#[cfg(test)]
mod tests {
    use core::hash::BuildHasher;

    use super::*;
    use crate::{
        Builder, ProgramPoint, SourceSpan,
        dialects::debuginfo::{DIBuilder, attributes::Variable},
        interner::Symbol,
    };

    #[test]
    fn clustered_capture_departures_do_not_hide_allocated_buckets() {
        let (test, anchor, value) = super::super::tests::fixture();
        let context = test.context_rc();
        let region = anchor.parent().unwrap().parent().unwrap();
        let mut builder = context.clone().builder();
        let crowded = builder.create_block(region, None, &[]);
        let destination = builder.create_block(region, None, &[]);
        builder.set_insertion_point_before(anchor);
        let hash_builder = *FxHashSet::<OperationRef>::default().hasher();
        let mut group = None;
        let mut selected = 0;
        // Fill one table with captures whose initial buckets cluster in a small group. Removing
        // its interior then leaves tombstones, reducing capacity() without releasing allocation.
        for _ in 0..20000 {
            let mut record = builder
                .debug_value(
                    value,
                    Variable::new(Symbol::intern("clustered"), Symbol::intern("test.rs"), 1, None),
                    SourceSpan::UNKNOWN,
                )
                .unwrap()
                .as_operation_ref();
            let hash = hash_builder.hash_one(record);
            let group = *group.get_or_insert(hash);
            if hash.wrapping_sub(group) & 127 < 16 {
                record.borrow_mut().move_to(ProgramPoint::at_end_of(crowded));
                selected += 1;
                if selected == 112 {
                    break;
                }
            }
        }
        assert_eq!(selected, 112);
        let captures = context.debug_info_mutations.borrow().observed.owners
            [&CaptureOwner::Block(crowded)]
            .records
            .iter()
            .copied()
            .collect::<alloc::vec::Vec<_>>();
        for mut record in captures.into_iter().skip(8).take(96) {
            record.borrow_mut().move_to(ProgramPoint::at_end_of(destination));
        }
        let state = context.debug_info_mutations.borrow();
        let bucket = &state.observed.owners[&CaptureOwner::Block(crowded)];
        let bytes = bucket.records.allocation_size();
        std::eprintln!(
            "clustered departures: live={}, reported capacity={}, allocated bytes={bytes}",
            bucket.records.len(),
            bucket.records.capacity()
        );
        assert_eq!(bucket.records.len(), 16);
        assert!(
            bytes <= 4 * core::mem::size_of::<OperationRef>() * 16,
            "tombstones concealed a sparse allocation"
        );
    }
}
