/// A result type used to indicatee if a change happened.
///
/// Supports boolean operations, with `Changed` representing a `true` value
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ChangeResult {
    Unchanged,
    Changed,
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
