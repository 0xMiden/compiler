use core::fmt;

use super::*;
use crate::{OpOperandRange, OpOperandRangeMut};

/// This struct represents owned region successor metadata
#[derive(Clone)]
pub struct RegionSuccessorInfo {
    pub successor: RegionBranchPoint,
    #[allow(unused)]
    pub(crate) key: Option<core::ptr::NonNull<()>>,
    pub(crate) operand_group: u8,
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
    pub dest: RegionBranchPoint,
    pub arguments: OpOperandRange<'a>,
}
impl<'a> RegionSuccessor<'a> {
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
    pub fn successor_inputs(&self) -> &OpOperandRange<'a> {
        &self.arguments
    }
}
impl fmt::Debug for RegionSuccessor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegionSuccessor")
            .field("dest", &self.dest)
            .field_with("arguments", |f| f.debug_list().entries(self.arguments.iter()).finish())
            .finish()
    }
}

/// The mutable version of [RegionSuccessor]
pub struct RegionSuccessorMut<'a> {
    pub dest: RegionBranchPoint,
    pub arguments: OpOperandRangeMut<'a>,
}
impl<'a> RegionSuccessorMut<'a> {
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
    pub fn successor_inputs(&mut self) -> &mut OpOperandRangeMut<'a> {
        &mut self.arguments
    }

    pub fn into_successor_inputs(self) -> OpOperandRangeMut<'a> {
        self.arguments
    }
}
impl fmt::Debug for RegionSuccessorMut<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegionSuccessorMut")
            .field("dest", &self.dest)
            .field_with("arguments", |f| f.debug_list().entries(self.arguments.iter()).finish())
            .finish()
    }
}
