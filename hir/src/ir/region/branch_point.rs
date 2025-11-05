use core::fmt;

use super::*;

/// This type represents a point being branched from in the methods of `RegionBranchOpInterface`.
///
/// One can branch from one of two different kinds of places:
///
/// * The parent operation (i.e. the op implementing `RegionBranchOpInterface`).
/// * A region within the parent operation (where the parent implements `RegionBranchOpInterface`).
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum RegionBranchPoint {
    /// A branch from the current operation to one of its regions
    Parent,
    /// A branch from the given region, within a parent `RegionBranchOpInterface` op
    Child(RegionRef),
}
impl fmt::Display for RegionBranchPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parent => f.write_str("parent"),
            Self::Child(ref region) => {
                write!(f, "child({})", region.borrow().region_number())
            }
        }
    }
}
impl fmt::Debug for RegionBranchPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parent => f.write_str("Parent"),
            Self::Child(ref region) => {
                f.debug_tuple("Child").field(&format_args!("{region:p}")).finish()
            }
        }
    }
}
impl RegionBranchPoint {
    /// Returns true if branching from the parent op.
    #[inline]
    pub fn is_parent(&self) -> bool {
        matches!(self, Self::Parent)
    }

    /// Returns the region if branching from a region, otherwise `None`.
    pub fn region(&self) -> Option<RegionRef> {
        match self {
            Self::Child(region) => Some(*region),
            Self::Parent => None,
        }
    }
}
impl<'a> From<RegionSuccessor<'a>> for RegionBranchPoint {
    fn from(succ: RegionSuccessor<'a>) -> Self {
        match succ.into_successor() {
            None => Self::Parent,
            Some(succ) => Self::Child(succ),
        }
    }
}
