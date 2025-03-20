use core::fmt;

/// A result type used to indicatee if a change happened.
///
/// Supports boolean operations, with `Changed` representing a `true` value
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ChangeResult {
    Unchanged,
    Changed,
}

impl ChangeResult {
    #[inline]
    pub fn changed(&self) -> bool {
        matches!(self, Self::Changed)
    }
}

impl fmt::Display for ChangeResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.changed() {
            f.write_str("changed")
        } else {
            f.write_str("unchanged")
        }
    }
}

impl core::ops::BitOr for ChangeResult {
    type Output = ChangeResult;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        if matches!(self, Self::Changed) {
            self
        } else {
            rhs
        }
    }
}
impl core::ops::BitAnd for ChangeResult {
    type Output = ChangeResult;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        if matches!(self, Self::Unchanged) {
            self
        } else {
            rhs
        }
    }
}
impl core::ops::BitOrAssign for ChangeResult {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}
impl core::ops::BitAndAssign for ChangeResult {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        *self = *self & rhs;
    }
}
impl From<ChangeResult> for bool {
    #[inline]
    fn from(value: ChangeResult) -> Self {
        value.changed()
    }
}
