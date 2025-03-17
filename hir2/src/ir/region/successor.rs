use core::fmt;

use self::value::BlockArgumentRange;
use super::*;
use crate::ValueRange;

/// This struct represents the metadata about a [RegionBranchOpInterface] successor, without
/// borrowing the op or the successor.
#[derive(Debug, Clone)]
pub enum RegionSuccessorInfo {
    /// We're entering the given region, and its successor operands are the block arguments of its
    /// entry block.
    Entering(RegionRef),
    /// We're exiting/returning to the parent op from one of its child regions.
    ///
    /// The given result group index is used to obtain the successor operands from the results of
    /// the terminator operation which transfers control to the parent.
    Returning(SmallVec<[ValueRef; 2]>),
}
impl RegionSuccessorInfo {
    pub fn successor(&self) -> RegionBranchPoint {
        match self {
            Self::Entering(region) => RegionBranchPoint::Child(*region),
            Self::Returning(_) => RegionBranchPoint::Parent,
        }
    }
}

/// A [RegionSuccessor] represents the successor of a region.
///
///
/// A region successor can either be another region, or the parent operation. If the successor is a
/// region, this class represents the destination region, as well as a set of arguments from that
/// region that will be populated when control flows into the region. If the successor is the parent
/// operation, this class represents an optional set of results that will be populated when control
/// returns to the parent operation.
///
/// This interface assumes that the values from the current region that are used to populate the
/// successor inputs are the operands of the return-like terminator operations in the blocks within
/// this region.
pub struct RegionSuccessor<'a> {
    dest: RegionBranchPoint,
    arguments: ValueRange<'a>,
}

impl<'a> RegionSuccessor<'a> {
    /// Creates a [RegionSuccessor] representing a branch to `dest` with `arguments`
    pub fn new(dest: RegionBranchPoint, arguments: impl Into<ValueRange<'a>>) -> Self {
        Self {
            dest,
            arguments: arguments.into(),
        }
    }

    /// Creates a [RegionSuccessor] representing a branch to another region of the parent operation.
    pub fn child(region: RegionRef, inputs: BlockArgumentRange<'a>) -> Self {
        Self {
            dest: RegionBranchPoint::Child(region),
            arguments: inputs.into(),
        }
    }

    /// Creates a [RegionSuccessor] representing a branch to/out of the parent operation.
    pub fn parent(inputs: OpResultRange<'a>) -> Self {
        Self {
            dest: RegionBranchPoint::Parent,
            arguments: inputs.into(),
        }
    }

    /// Get the underlying [RegionBranchPoint]
    pub fn branch_point(&self) -> &RegionBranchPoint {
        &self.dest
    }

    /// Returns true if the successor is the parent op
    pub fn is_parent(&self) -> bool {
        self.dest.is_parent()
    }

    pub fn successor(&self) -> Option<RegionRef> {
        self.dest.region()
    }

    pub fn into_successor(self) -> Option<RegionRef> {
        self.dest.region()
    }

    /// Return the inputs to the successor that are remapped by the exit values of the current
    /// region.
    pub fn successor_inputs(&self) -> &ValueRange<'a> {
        &self.arguments
    }
}

impl fmt::Debug for RegionSuccessor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegionSuccessor")
            .field("dest", &self.dest)
            .field_with("arguments", |f| {
                let mut list = f.debug_list();
                for operand in self.arguments.iter() {
                    list.entry(&operand.borrow());
                }
                list.finish()
            })
            .finish()
    }
}

pub struct RegionSuccessorIter<'a> {
    // TODO(pauls): See if we can get rid of [RegionSuccessorInfo] entirely and just use
    // [RegionSuccessor]
    #[allow(unused)]
    op: &'a Operation,
    successors: SmallVec<[RegionSuccessorInfo; 2]>,
    index: usize,
}
impl<'a> RegionSuccessorIter<'a> {
    pub fn new(
        op: &'a Operation,
        successors: impl IntoIterator<Item = RegionSuccessorInfo>,
    ) -> Self {
        Self {
            op,
            successors: SmallVec::from_iter(successors),
            index: 0,
        }
    }

    pub fn empty(op: &'a Operation) -> Self {
        Self {
            op,
            successors: Default::default(),
            index: 0,
        }
    }

    pub fn get(&self, index: usize) -> Option<RegionSuccessor<'a>> {
        self.successors.get(index).map(|info| match info {
            RegionSuccessorInfo::Entering(region) => {
                let operands = ValueRange::Owned(
                    region
                        .borrow()
                        .entry()
                        .arguments()
                        .iter()
                        .map(|arg| arg.borrow().as_value_ref())
                        .collect(),
                );
                RegionSuccessor::new(RegionBranchPoint::Child(*region), operands)
            }
            RegionSuccessorInfo::Returning(results) => {
                RegionSuccessor::new(RegionBranchPoint::Parent, results.clone())
            }
        })
    }

    pub fn into_successor_infos(self) -> SmallVec<[RegionSuccessorInfo; 2]> {
        self.successors
    }
}
impl core::iter::FusedIterator for RegionSuccessorIter<'_> {}
impl ExactSizeIterator for RegionSuccessorIter<'_> {
    fn len(&self) -> usize {
        self.successors.len()
    }
}
impl<'a> Iterator for RegionSuccessorIter<'a> {
    type Item = RegionSuccessor<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.successors.len() {
            return None;
        }

        let next = self.get(self.index)?;

        self.index += 1;

        Some(next)
    }
}
