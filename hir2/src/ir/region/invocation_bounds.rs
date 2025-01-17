use core::ops::{Bound, RangeBounds};

/// This type represents upper and lower bounds on the number of times a region of a
/// `RegionBranchOpInterface` op can be invoked. The lower bound is at least zero, but the upper
/// bound may not be known.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum InvocationBounds {
    /// The region can be invoked an unknown number of times, possibly never.
    #[default]
    Unknown,
    /// The region can never be invoked
    Never,
    /// The region can be invoked exactly N times
    Exact(u32),
    /// The region can be invoked any number of times in the given range
    Variable { min: u32, max: u32 },
    /// The region can be invoked at least N times, but an unknown number of times beyond that.
    AtLeastN(u32),
    /// The region can be invoked any number of times up to N
    NoMoreThan(u32),
}
impl InvocationBounds {
    #[inline]
    pub fn new(bounds: impl Into<Self>) -> Self {
        bounds.into()
    }

    #[inline]
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    pub fn min(&self) -> Bound<&u32> {
        self.start_bound()
    }

    pub fn max(&self) -> Bound<&u32> {
        self.end_bound()
    }
}
impl From<u32> for InvocationBounds {
    fn from(value: u32) -> Self {
        if value == 0 {
            Self::Never
        } else {
            Self::Exact(value)
        }
    }
}
impl From<core::ops::Range<u32>> for InvocationBounds {
    fn from(range: core::ops::Range<u32>) -> Self {
        if range.start == range.end {
            Self::Never
        } else if range.end == range.start + 1 {
            Self::Exact(range.start)
        } else {
            assert!(range.start < range.end);
            Self::Variable {
                min: range.start,
                max: range.end,
            }
        }
    }
}
impl From<core::ops::RangeFrom<u32>> for InvocationBounds {
    fn from(value: core::ops::RangeFrom<u32>) -> Self {
        if value.start == 0 {
            Self::Unknown
        } else {
            Self::AtLeastN(value.start)
        }
    }
}
impl From<core::ops::RangeTo<u32>> for InvocationBounds {
    fn from(value: core::ops::RangeTo<u32>) -> Self {
        if value.end == 1 {
            Self::Never
        } else if value.end == u32::MAX {
            Self::Unknown
        } else {
            Self::NoMoreThan(value.end - 1)
        }
    }
}
impl From<core::ops::RangeFull> for InvocationBounds {
    fn from(_value: core::ops::RangeFull) -> Self {
        Self::Unknown
    }
}
impl From<core::ops::RangeInclusive<u32>> for InvocationBounds {
    fn from(range: core::ops::RangeInclusive<u32>) -> Self {
        let (start, end) = range.into_inner();
        if start == 0 && end == 0 {
            Self::Never
        } else if start == end {
            Self::Exact(start)
        } else {
            Self::Variable {
                min: start,
                max: end + 1,
            }
        }
    }
}
impl From<core::ops::RangeToInclusive<u32>> for InvocationBounds {
    fn from(range: core::ops::RangeToInclusive<u32>) -> Self {
        if range.end == 0 {
            Self::Never
        } else {
            Self::NoMoreThan(range.end)
        }
    }
}
impl RangeBounds<u32> for InvocationBounds {
    fn start_bound(&self) -> Bound<&u32> {
        match self {
            Self::Unknown | Self::NoMoreThan(_) => Bound::Unbounded,
            Self::Never => Bound::Excluded(&0),
            Self::Exact(n) | Self::Variable { min: n, .. } => Bound::Included(n),
            Self::AtLeastN(n) => Bound::Included(n),
        }
    }

    fn end_bound(&self) -> Bound<&u32> {
        match self {
            Self::Unknown | Self::AtLeastN(_) => Bound::Unbounded,
            Self::Never => Bound::Excluded(&0),
            Self::Exact(n) | Self::Variable { max: n, .. } => Bound::Excluded(n),
            Self::NoMoreThan(n) => Bound::Excluded(n),
        }
    }
}
