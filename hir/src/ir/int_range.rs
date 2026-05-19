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
            Type::Felt => Self {
                width: 64,
                umin: 0,
                umax: (Felt::ORDER - 1) as u128,
                smin: 0,
                smax: (Felt::ORDER - 1) as i128,
            },
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
                self.umin >= constraint.umin
                    && self.umax <= constraint.umax
                    && self.smin >= constraint.smin
                    && self.smax <= constraint.smax
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

/// The integer range lattice value attached to an SSA value.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct IntegerValueRange {
    value: Option<ConstantIntRange>,
}

impl IntegerValueRange {
    /// Create an uninitialized lattice value.
    #[inline]
    pub const fn uninitialized() -> Self {
        Self { value: None }
    }

    /// Create an initialized lattice value from `range`.
    #[inline]
    pub const fn new(range: ConstantIntRange) -> Self {
        Self { value: Some(range) }
    }

    /// Create a top value for the range of `ty`.
    #[inline]
    pub fn max_for_type(ty: &Type) -> Self {
        Self::new(ConstantIntRange::max_for_type(ty))
    }

    /// Create an exact range for `value`.
    #[inline]
    pub fn constant(value: Immediate) -> Self {
        Self::new(ConstantIntRange::constant(value))
    }

    /// Return true if this lattice value has not been initialized.
    #[inline]
    pub const fn is_uninitialized(&self) -> bool {
        self.value.is_none()
    }

    /// Return the underlying range if initialized.
    #[inline]
    pub const fn range(&self) -> Option<&ConstantIntRange> {
        self.value.as_ref()
    }

    /// Return this range intersected with `constraint`.
    pub fn constrained_by(&self, constraint: &ValueRangeConstraint) -> Self {
        let constraint = ConstantIntRange::max_for_type(constraint.ty());
        match self.value {
            None => Self::new(constraint),
            Some(current) if current.width == constraint.width => {
                Self::new(current.intersection(&constraint))
            }
            Some(current) if !current.is_untracked() && !constraint.is_untracked() => {
                let umin = current.umin.max(constraint.umin);
                let umax = current.umax.min(constraint.umax);
                if umin <= umax {
                    Self::new(ConstantIntRange::from_unsigned(constraint.width, umin, umax))
                } else {
                    // A checked assertion/cast with no overlapping runtime values would trap. The
                    // result can only exist on paths where the constraint holds.
                    Self::new(constraint)
                }
            }
            Some(_) => Self::new(constraint),
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
        match self.value {
            None => f.write_str("uninitialized"),
            Some(range) => fmt::Display::fmt(&range, f),
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
