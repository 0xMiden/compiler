use core::fmt;

use super::{Felt, Immediate, Type};
use crate::traits::ValueRangeConstraint;

/// Bounds on an integer value, tracked using both signed and unsigned interpretations.
///
/// This mirrors MLIR's `ConstantIntRanges`, but is intentionally bounded to the integer widths HIR
/// can currently materialize as immediates. Values with unsupported or non-integer types are
/// represented by `width == 0`.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct ConstantIntRange {
    width: u16,
    umin: u128,
    umax: u128,
    smin: i128,
    smax: i128,
}

impl ConstantIntRange {
    /// Return an untracked/non-integer range.
    #[inline]
    pub const fn untracked() -> Self {
        Self {
            width: 0,
            umin: 0,
            umax: 0,
            smin: 0,
            smax: 0,
        }
    }

    /// Return true if this range represents a value that cannot be tracked precisely.
    #[inline]
    pub const fn is_untracked(&self) -> bool {
        self.width == 0
    }

    /// Return the storage width used for this range, or 0 if untracked.
    #[inline]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// The minimum unsigned bound.
    #[inline]
    pub const fn umin(&self) -> u128 {
        self.umin
    }

    /// The maximum unsigned bound.
    #[inline]
    pub const fn umax(&self) -> u128 {
        self.umax
    }

    /// The minimum signed bound.
    #[inline]
    pub const fn smin(&self) -> i128 {
        self.smin
    }

    /// The maximum signed bound.
    #[inline]
    pub const fn smax(&self) -> i128 {
        self.smax
    }

    /// Return the conservative range for all valid values of `ty`.
    pub fn max_for_type(ty: &Type) -> Self {
        match ty {
            Type::I1 => Self::from_unsigned(1, 0, 1),
            Type::U8 => Self::from_unsigned(8, 0, u8::MAX as u128),
            Type::U16 => Self::from_unsigned(16, 0, u16::MAX as u128),
            Type::U32 | Type::Ptr(_) | Type::Function(_) => {
                Self::from_unsigned(32, 0, u32::MAX as u128)
            }
            Type::U64 => Self::from_unsigned(64, 0, u64::MAX as u128),
            Type::U128 => Self::from_unsigned(128, 0, u128::MAX),
            Type::I8 => Self::from_signed(8, i8::MIN as i128, i8::MAX as i128),
            Type::I16 => Self::from_signed(16, i16::MIN as i128, i16::MAX as i128),
            Type::I32 => Self::from_signed(32, i32::MIN as i128, i32::MAX as i128),
            Type::I64 => Self::from_signed(64, i64::MIN as i128, i64::MAX as i128),
            Type::I128 => Self::from_signed(128, i128::MIN, i128::MAX),
            Type::Felt => Self::from_unsigned(64, 0, (Felt::ORDER - 1) as u128),
            _ => Self::untracked(),
        }
    }

    /// Return the exact range for an integer immediate.
    pub fn constant(value: Immediate) -> Self {
        match value {
            Immediate::I1(value) => Self::from_unsigned(1, value as u128, value as u128),
            Immediate::U8(value) => Self::from_unsigned(8, value as u128, value as u128),
            Immediate::U16(value) => Self::from_unsigned(16, value as u128, value as u128),
            Immediate::U32(value) => Self::from_unsigned(32, value as u128, value as u128),
            Immediate::U64(value) => Self::from_unsigned(64, value as u128, value as u128),
            Immediate::U128(value) => Self::from_unsigned(128, value, value),
            Immediate::I8(value) => Self::from_signed(8, value as i128, value as i128),
            Immediate::I16(value) => Self::from_signed(16, value as i128, value as i128),
            Immediate::I32(value) => Self::from_signed(32, value as i128, value as i128),
            Immediate::I64(value) => Self::from_signed(64, value as i128, value as i128),
            Immediate::I128(value) => Self::from_signed(128, value, value),
            Immediate::Felt(value) => {
                let value = value.as_canonical_u64() as u128;
                Self::from_unsigned(64, value, value)
            }
            Immediate::F64(_) => Self::untracked(),
        }
    }

    /// Return a range with unsigned semantic bounds.
    pub fn from_unsigned(width: u16, umin: u128, umax: u128) -> Self {
        if width == 0 || width > 128 {
            return Self::untracked();
        }

        let max = unsigned_max(width);
        let umin = umin.min(max);
        let umax = umax.min(max);
        let signed_max = signed_max(width);
        let (smin, smax) = if umax <= signed_max as u128 {
            (umin as i128, umax as i128)
        } else if umin > signed_max as u128 {
            (unsigned_to_signed(width, umin), unsigned_to_signed(width, umax))
        } else {
            (signed_min(width), signed_max)
        };

        Self {
            width,
            umin,
            umax,
            smin,
            smax,
        }
    }

    /// Return a range with signed semantic bounds.
    pub fn from_signed(width: u16, smin: i128, smax: i128) -> Self {
        if width == 0 || width > 128 {
            return Self::untracked();
        }

        let min = signed_min(width);
        let max = signed_max(width);
        let smin = smin.max(min);
        let smax = smax.min(max);
        let (umin, umax) = if smin >= 0 {
            (smin as u128, smax as u128)
        } else if smax < 0 {
            (signed_to_unsigned(width, smin), signed_to_unsigned(width, smax))
        } else {
            (0, unsigned_max(width))
        };

        Self {
            width,
            umin,
            umax,
            smin,
            smax,
        }
    }

    /// Compute the least upper bound of two ranges.
    pub fn union(&self, other: &Self) -> Self {
        if self.is_untracked() {
            return *self;
        }
        if other.is_untracked() {
            return *other;
        }
        if self.width != other.width {
            return Self::untracked();
        }

        Self {
            width: self.width,
            umin: self.umin.min(other.umin),
            umax: self.umax.max(other.umax),
            smin: self.smin.min(other.smin),
            smax: self.smax.max(other.smax),
        }
    }

    /// Widen expanding bounds to the limits of their representation, ensuring that
    /// repeated loop-carried joins reach a fixed point in a bounded number of steps.
    pub fn widen(&self, other: &Self) -> Self {
        let joined = self.union(other);
        if joined.is_untracked() {
            return joined;
        }
        Self {
            width: joined.width,
            umin: if joined.umin < self.umin {
                0
            } else {
                joined.umin
            },
            umax: if joined.umax > self.umax {
                unsigned_max(joined.width)
            } else {
                joined.umax
            },
            smin: if joined.smin < self.smin {
                signed_min(joined.width)
            } else {
                joined.smin
            },
            smax: if joined.smax > self.smax {
                signed_max(joined.width)
            } else {
                joined.smax
            },
        }
    }

    /// Compute the greatest lower bound of two ranges.
    pub fn intersection(&self, other: &Self) -> Self {
        if self.is_untracked() {
            return *self;
        }
        if other.is_untracked() {
            return *other;
        }
        if self.width != other.width {
            return Self::untracked();
        }

        let umin = self.umin.max(other.umin);
        let umax = self.umax.min(other.umax);
        let smin = self.smin.max(other.smin);
        let smax = self.smax.min(other.smax);
        if umin > umax || smin > smax {
            return Self::untracked();
        }

        Self {
            width: self.width,
            umin,
            umax,
            smin,
            smax,
        }
    }

    /// Infer the result range of an addition when overflow is statically impossible.
    pub fn add_without_overflow(&self, other: &Self) -> Option<Self> {
        if self.is_untracked() || other.is_untracked() || self.width != other.width {
            return None;
        }

        let umin = self.umin.checked_add(other.umin)?;
        let umax = self.umax.checked_add(other.umax)?;
        if umax > unsigned_max(self.width) {
            return None;
        }

        let smin = self.smin.checked_add(other.smin)?;
        let smax = self.smax.checked_add(other.smax)?;
        if smin < signed_min(self.width) || smax > signed_max(self.width) {
            return None;
        }

        Some(Self {
            width: self.width,
            umin,
            umax,
            smin,
            smax,
        })
    }

    /// Return true if the signed lower bound is known to be non-negative.
    #[inline]
    pub const fn is_statically_non_negative(&self) -> bool {
        !self.is_untracked() && self.smin >= 0
    }

    /// Return true if this range is fully contained by `constraint`.
    pub fn satisfies(&self, constraint: &ValueRangeConstraint) -> bool {
        match constraint {
            ValueRangeConstraint::Type(ty) => {
                let constraint = Self::max_for_type(ty);
                if self.is_untracked() || constraint.is_untracked() {
                    return false;
                }
                let unsigned_fits = self.umin >= constraint.umin && self.umax <= constraint.umax;
                if matches!(ty, Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128) {
                    // Signed bounds describe bit patterns at a particular width. Do not
                    // interpret wider bounds as facts about a narrower signed value.
                    unsigned_fits
                        && self.width == constraint.width
                        && self.smin >= constraint.smin
                        && self.smax <= constraint.smax
                } else {
                    unsigned_fits
                }
            }
        }
    }
}

impl fmt::Debug for ConstantIntRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for ConstantIntRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_untracked() {
            f.write_str("untracked")
        } else {
            write!(
                f,
                "u[{}, {}] s[{}, {}] width {}",
                self.umin, self.umax, self.smin, self.smax, self.width
            )
        }
    }
}

/// Whether an integer range is a proven fact or only an assumption derived from type fallback.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum IntegerRangeConfidence {
    /// The range is derived from operation semantics or a checked runtime refinement.
    Proven,
    /// The range is only the conservative range of the value's static type.
    Assumed,
}

impl IntegerRangeConfidence {
    #[inline]
    pub const fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Proven, Self::Proven) => Self::Proven,
            _ => Self::Assumed,
        }
    }
}

/// The integer range lattice value attached to an SSA value.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct IntegerValueRange {
    value: Option<ConstantIntRange>,
    confidence: Option<IntegerRangeConfidence>,
}

impl IntegerValueRange {
    /// Create an uninitialized lattice value.
    #[inline]
    pub const fn uninitialized() -> Self {
        Self {
            value: None,
            confidence: None,
        }
    }

    /// Create a proven initialized lattice value from `range`.
    #[inline]
    pub const fn new(range: ConstantIntRange) -> Self {
        Self::proven(range)
    }

    /// Create a proven initialized lattice value from `range`.
    #[inline]
    pub const fn proven(range: ConstantIntRange) -> Self {
        Self {
            value: Some(range),
            confidence: Some(IntegerRangeConfidence::Proven),
        }
    }

    /// Create an assumed initialized lattice value from `range`.
    #[inline]
    pub const fn assumed(range: ConstantIntRange) -> Self {
        Self {
            value: Some(range),
            confidence: Some(IntegerRangeConfidence::Assumed),
        }
    }

    /// Create an initialized lattice value from `range` and `confidence`.
    #[inline]
    pub const fn with_confidence(
        range: ConstantIntRange,
        confidence: IntegerRangeConfidence,
    ) -> Self {
        Self {
            value: Some(range),
            confidence: Some(confidence),
        }
    }

    /// Create an assumed top value for the range of `ty`.
    #[inline]
    pub fn max_for_type(ty: &Type) -> Self {
        Self::assumed(ConstantIntRange::max_for_type(ty))
    }

    /// Create a proven exact range for `value`.
    #[inline]
    pub fn constant(value: Immediate) -> Self {
        Self::proven(ConstantIntRange::constant(value))
    }

    /// Return true if this lattice value has not been initialized.
    #[inline]
    pub const fn is_uninitialized(&self) -> bool {
        self.value.is_none()
    }

    /// Return the confidence for this range, if initialized.
    #[inline]
    pub const fn confidence(&self) -> Option<IntegerRangeConfidence> {
        self.confidence
    }

    /// Return true if this lattice value is a proven range fact.
    #[inline]
    pub const fn is_proven(&self) -> bool {
        matches!(self.confidence, Some(IntegerRangeConfidence::Proven))
    }

    /// Return the underlying range if initialized.
    #[inline]
    pub const fn range(&self) -> Option<&ConstantIntRange> {
        self.value.as_ref()
    }

    /// Return this range intersected with `constraint`.
    pub fn constrained_by(&self, constraint: &ValueRangeConstraint) -> Self {
        let constraint = ConstantIntRange::max_for_type(constraint.ty());
        // A runtime check proves its own constraint, not unchecked assumptions
        // inherited from the input's static type.
        if !self.is_proven() {
            return Self::proven(constraint);
        }
        match self.value {
            None => Self::proven(constraint),
            Some(current) if current.width == constraint.width => {
                Self::proven(current.intersection(&constraint))
            }
            Some(current) if !current.is_untracked() && !constraint.is_untracked() => {
                let umin = current.umin.max(constraint.umin);
                let umax = current.umax.min(constraint.umax);
                if umin <= umax {
                    Self::proven(ConstantIntRange::from_unsigned(constraint.width, umin, umax))
                } else {
                    // A checked assertion/cast with no overlapping runtime values would trap. The
                    // result can only exist on paths where the constraint holds.
                    Self::proven(constraint)
                }
            }
            Some(_) => Self::proven(constraint),
        }
    }
}

impl fmt::Debug for IntegerValueRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for IntegerValueRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.value, self.confidence) {
            (None, _) => f.write_str("uninitialized"),
            (Some(range), confidence) => {
                fmt::Display::fmt(&range, f)?;
                match confidence {
                    Some(IntegerRangeConfidence::Proven) => f.write_str(" proven"),
                    Some(IntegerRangeConfidence::Assumed) => f.write_str(" assumed"),
                    None => Ok(()),
                }
            }
        }
    }
}

#[inline]
const fn unsigned_max(width: u16) -> u128 {
    if width >= 128 {
        u128::MAX
    } else {
        (1u128 << width) - 1
    }
}

#[inline]
const fn signed_min(width: u16) -> i128 {
    if width == 0 {
        0
    } else if width >= 128 {
        i128::MIN
    } else {
        -(1i128 << (width - 1))
    }
}

#[inline]
const fn signed_max(width: u16) -> i128 {
    if width == 0 {
        0
    } else if width >= 128 {
        i128::MAX
    } else {
        (1i128 << (width - 1)) - 1
    }
}

#[inline]
const fn unsigned_to_signed(width: u16, value: u128) -> i128 {
    if width >= 128 {
        value as i128
    } else {
        let sign_bit = 1u128 << (width - 1);
        if value & sign_bit == 0 {
            value as i128
        } else {
            (value as i128) - (1i128 << width)
        }
    }
}

#[inline]
const fn signed_to_unsigned(width: u16, value: i128) -> u128 {
    if width >= 128 || value >= 0 {
        value as u128
    } else {
        (1u128 << width) - value.unsigned_abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_checked_refinement_does_not_prove_assumed_bounds() {
        let assumed = IntegerValueRange::max_for_type(&Type::U8);
        let refined = assumed.constrained_by(&ValueRangeConstraint::Type(Type::U32));
        assert!(refined.is_proven());
        assert_eq!(refined.range().unwrap().umax(), u32::MAX as u128);
    }

    #[test]
    fn regression_unsigned_constraint_accepts_wider_representation() {
        let range = ConstantIntRange::constant(Immediate::Felt(Felt::new(1u64 << 31).unwrap()));
        assert!(range.satisfies(&ValueRangeConstraint::Type(Type::U32)));
        assert!(!range.satisfies(&ValueRangeConstraint::Type(Type::U16)));
    }

    #[test]
    fn regression_felt_type_range_contains_all_felt_constants() {
        let top = ConstantIntRange::max_for_type(&Type::Felt);
        let constant =
            ConstantIntRange::constant(Immediate::Felt(Felt::new(Felt::ORDER - 1).unwrap()));
        assert_eq!(top.union(&constant), top);
    }
}
