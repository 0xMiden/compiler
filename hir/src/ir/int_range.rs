use core::fmt;

use super::{Felt, Immediate, Type};
use crate::traits::ValueRangeConstraint;

/// Bounds on an integer value, tracked using both signed and unsigned interpretations.
///
/// Unsigned bounds describe the stored raw representation: an N-bit integer, including a signed
/// integer, is zero-extended to its storage width. Signed bounds interpret the same bit pattern
/// at `width`; they are only meaningful for signed ordering at that width.
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

    /// Return an exact boolean in the raw representation of `ty`.
    pub fn exact_bool(ty: &Type, value: bool) -> Self {
        let width = Self::max_for_type(ty).width();
        let value = u128::from(value);
        Self::from_unsigned(width, value, value)
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

    /// Intersect two ranges, returning `None` for an empty intersection or incompatible widths.
    /// An untracked range imposes no bounds, so it is the identity for intersection.
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        if self.is_untracked() {
            return Some(*other);
        }
        if other.is_untracked() {
            return Some(*self);
        }
        if self.width != other.width {
            return None;
        }

        let umin = self.umin.max(other.umin);
        let umax = self.umax.min(other.umax);
        let smin = self.smin.max(other.smin);
        let smax = self.smax.min(other.smax);
        if umin > umax || smin > smax {
            return None;
        }

        // Intersect each sign's raw representation separately. Overlapping unsigned and signed
        // bounds alone do not guarantee a common bit pattern.
        let positive = (smax >= 0).then(|| (umin.max(smin.max(0) as u128), umax.min(smax as u128)));
        let negative = (smin < 0).then(|| {
            (
                umin.max(signed_to_unsigned(self.width, smin)),
                umax.min(signed_to_unsigned(self.width, smax.min(-1))),
            )
        });
        [positive, negative]
            .into_iter()
            .flatten()
            .filter(|(min, max)| min <= max)
            .map(|(min, max)| Self::from_unsigned(self.width, min, max))
            .reduce(|lhs, rhs| lhs.union(&rhs))
    }

    /// Infer addition when the raw unsigned representation cannot overflow.
    pub fn add_unsigned_without_overflow(&self, other: &Self) -> Option<Self> {
        if self.is_untracked() || other.is_untracked() || self.width != other.width {
            return None;
        }
        let umin = self.umin.checked_add(other.umin)?;
        let umax = self.umax.checked_add(other.umax)?;
        (umax <= unsigned_max(self.width)).then(|| Self::from_unsigned(self.width, umin, umax))
    }

    /// Infer addition when the signed interpretation cannot overflow.
    pub fn add_signed_without_overflow(&self, other: &Self) -> Option<Self> {
        if self.is_untracked() || other.is_untracked() || self.width != other.width {
            return None;
        }
        let smin = self.smin.checked_add(other.smin)?;
        let smax = self.smax.checked_add(other.smax)?;
        (smin >= signed_min(self.width) && smax <= signed_max(self.width))
            .then(|| Self::from_signed(self.width, smin, smax))
    }

    /// Infer bitwise wrapping addition from either non-overflowing interpretation.
    /// Each interpretation can independently constrain the same resulting bit pattern.
    pub fn add_wrapping(&self, other: &Self) -> Option<Self> {
        match (
            self.add_unsigned_without_overflow(other),
            self.add_signed_without_overflow(other),
        ) {
            (Some(unsigned), Some(signed)) => unsigned.intersection(&signed),
            (range, None) | (None, range) => range,
        }
    }

    /// Return true if this range is fully contained by `constraint`.
    pub fn satisfies(&self, constraint: &ValueRangeConstraint) -> bool {
        match constraint {
            ValueRangeConstraint::Type(ty) => {
                let constraint = Self::max_for_type(ty);
                if self.is_untracked() || constraint.is_untracked() {
                    return false;
                }
                self.umin >= constraint.umin && self.umax <= constraint.umax
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
    /// It contains every possible value, but may also contain unreachable values after widening.
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
    value: Option<(ConstantIntRange, IntegerRangeConfidence)>,
}

impl IntegerValueRange {
    /// Create an uninitialized lattice value.
    #[inline]
    pub const fn uninitialized() -> Self {
        Self { value: None }
    }

    /// Create a proven initialized lattice value from `range`.
    #[inline]
    pub const fn proven(range: ConstantIntRange) -> Self {
        Self::with_confidence(range, IntegerRangeConfidence::Proven)
    }

    /// Create an assumed initialized lattice value from `range`.
    #[inline]
    pub const fn assumed(range: ConstantIntRange) -> Self {
        Self::with_confidence(range, IntegerRangeConfidence::Assumed)
    }

    /// Create an initialized lattice value from `range` and `confidence`.
    #[inline]
    pub const fn with_confidence(
        range: ConstantIntRange,
        confidence: IntegerRangeConfidence,
    ) -> Self {
        Self {
            value: Some((range, confidence)),
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
        match self.value {
            Some((_, confidence)) => Some(confidence),
            None => None,
        }
    }

    /// Join operand confidence, waiting until both operands have initialized facts.
    #[inline]
    pub fn join_confidence(&self, other: &Self) -> Option<IntegerRangeConfidence> {
        Some(self.confidence()?.join(other.confidence()?))
    }

    /// Return true if this lattice value is a proven range fact.
    #[inline]
    pub const fn is_proven(&self) -> bool {
        matches!(self.confidence(), Some(IntegerRangeConfidence::Proven))
    }

    /// Return the underlying range if initialized.
    #[inline]
    pub const fn range(&self) -> Option<&ConstantIntRange> {
        match &self.value {
            Some((range, _)) => Some(range),
            None => None,
        }
    }

    /// Create a proven exact boolean in the raw representation of `ty`.
    pub fn exact_bool(ty: &Type, value: bool) -> Self {
        Self::proven(ConstantIntRange::exact_bool(ty, value))
    }

    /// Refine with an explicit runtime-checked range in the result's representation.
    /// Cross-width refinement does not model sign extension.
    pub fn refined_by(&self, range: &ConstantIntRange) -> Self {
        let Some((current, confidence)) = self.value else {
            return Self::uninitialized();
        };
        // A runtime check proves its own range, not assumptions inherited from the type.
        let narrowed = match confidence {
            IntegerRangeConfidence::Assumed => None,
            IntegerRangeConfidence::Proven if current.width == range.width => {
                current.intersection(range)
            }
            IntegerRangeConfidence::Proven if !current.is_untracked() && !range.is_untracked() => {
                let umin = current.umin.max(range.umin);
                let umax = current.umax.min(range.umax);
                (umin <= umax)
                    .then(|| ConstantIntRange::from_unsigned(range.width, umin, umax))
                    .and_then(|current| current.intersection(range))
            }
            IntegerRangeConfidence::Proven => None,
        };
        // Disjoint proven inputs trap. Any result that exists still satisfies the checked range.
        Self::proven(narrowed.unwrap_or(*range))
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
            Some((range, confidence)) => {
                fmt::Display::fmt(&range, f)?;
                match confidence {
                    IntegerRangeConfidence::Proven => f.write_str(" proven"),
                    IntegerRangeConfidence::Assumed => f.write_str(" assumed"),
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
        let refined = assumed.refined_by(&ConstantIntRange::max_for_type(&Type::U32));
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

    #[test]
    fn wrapping_add_uses_each_interpretation_independently() {
        let unsigned = ConstantIntRange::constant(Immediate::U32(0x7fff_ffff));
        let one = ConstantIntRange::constant(Immediate::U32(1));
        assert_eq!(
            unsigned.add_wrapping(&one),
            Some(ConstantIntRange::constant(Immediate::U32(0x8000_0000)))
        );
        let signed = ConstantIntRange::constant(Immediate::I32(-1));
        assert_eq!(signed.add_wrapping(&one), Some(ConstantIntRange::constant(Immediate::I32(0))));
        let straddling = ConstantIntRange::from_unsigned(32, 0, 0x8000_0000);
        assert_eq!(
            straddling.add_wrapping(&one),
            Some(ConstantIntRange::from_unsigned(32, 1, 0x8000_0001))
        );
    }

    #[test]
    fn checked_add_uses_the_selected_interpretation() {
        let max = ConstantIntRange::constant(Immediate::U32(0x7fff_ffff));
        let one = ConstantIntRange::constant(Immediate::U32(1));
        assert!(max.add_unsigned_without_overflow(&one).is_some());
        assert!(max.add_signed_without_overflow(&one).is_none());
        let minus_one = ConstantIntRange::constant(Immediate::I32(-1));
        assert!(minus_one.add_unsigned_without_overflow(&one).is_none());
        assert_eq!(
            minus_one.add_signed_without_overflow(&one),
            Some(ConstantIntRange::constant(Immediate::I32(0)))
        );
    }

    #[test]
    fn intersection_respects_top_empty_and_incompatible_ranges() {
        let range = ConstantIntRange::from_unsigned(8, 0, 5);
        assert_eq!(range.intersection(&ConstantIntRange::untracked()), Some(range));
        assert_eq!(ConstantIntRange::untracked().intersection(&range), Some(range));
        assert_eq!(range.intersection(&ConstantIntRange::from_unsigned(8, 6, 10)), None);
        assert_eq!(range.intersection(&ConstantIntRange::max_for_type(&Type::U16)), None);
        let lhs = ConstantIntRange::from_unsigned(8, 0, 50)
            .union(&ConstantIntRange::from_unsigned(8, 128, 200));
        let rhs = ConstantIntRange::from_unsigned(8, 60, 127)
            .union(&ConstantIntRange::from_unsigned(8, 206, 255));
        assert_eq!(lhs.intersection(&rhs), None);
    }

    #[test]
    fn refinement_waits_for_input_and_preserves_proven_bounds() {
        let range = ConstantIntRange::max_for_type(&Type::U32);
        assert!(IntegerValueRange::uninitialized().refined_by(&range).is_uninitialized());
        let precise = IntegerValueRange::proven(ConstantIntRange::from_unsigned(64, 0, 10));
        assert_eq!(
            precise.refined_by(&range).range(),
            Some(&ConstantIntRange::from_unsigned(32, 0, 10))
        );
        let disjoint = IntegerValueRange::constant(Immediate::U64(u64::MAX));
        assert_eq!(
            disjoint.refined_by(&range),
            IntegerValueRange::proven(ConstantIntRange::max_for_type(&Type::U32))
        );
    }

    #[test]
    fn explicit_refinement_discards_assumed_bounds() {
        let checked = ConstantIntRange::from_unsigned(32, 0, 2_147_483_647);
        for input in [
            ConstantIntRange::from_unsigned(8, 0, 3),
            ConstantIntRange::from_unsigned(32, 0, 3),
            ConstantIntRange::from_unsigned(64, 0, 3),
        ] {
            assert_eq!(
                IntegerValueRange::assumed(input).refined_by(&checked),
                IntegerValueRange::proven(checked)
            );
        }
        assert!(IntegerValueRange::uninitialized().refined_by(&checked).is_uninitialized());
    }

    #[test]
    fn cross_width_refinement_preserves_declared_signed_bounds() {
        // The declared raw interval spans all eight-bit values, but its signed bounds exclude
        // 6..=250. A wider input must not erase that part of the runtime check.
        let checked = ConstantIntRange::from_signed(8, -5, 5);
        for (min, max, expected) in [
            (0, 200, ConstantIntRange::from_unsigned(8, 0, 5)),
            (128, 255, ConstantIntRange::from_signed(8, -5, -1)),
        ] {
            let input = IntegerValueRange::proven(ConstantIntRange::from_unsigned(64, min, max));
            assert_eq!(input.refined_by(&checked), IntegerValueRange::proven(expected));
        }
    }

    #[test]
    fn signed_constraints_check_the_zero_extended_representation() {
        for (ty, width) in [(Type::I8, 8), (Type::I16, 16), (Type::I32, 32)] {
            let constraint = ValueRangeConstraint::Type(ty);
            let max = (1u128 << width) - 1;
            assert!(ConstantIntRange::from_unsigned(64, 0, max).satisfies(&constraint));
            assert!(!ConstantIntRange::from_unsigned(64, 0, max + 1).satisfies(&constraint));
        }
    }

    #[test]
    fn confidence_join_waits_and_never_upgrades_assumptions() {
        let proven = IntegerValueRange::exact_bool(&Type::I1, true);
        let assumed = IntegerValueRange::max_for_type(&Type::I1);
        assert_eq!(proven.join_confidence(&proven), Some(IntegerRangeConfidence::Proven));
        assert_eq!(proven.join_confidence(&assumed), Some(IntegerRangeConfidence::Assumed));
        assert_eq!(assumed.join_confidence(&proven), Some(IntegerRangeConfidence::Assumed));
        assert_eq!(proven.join_confidence(&IntegerValueRange::uninitialized()), None);
        assert_eq!(proven.range(), Some(&ConstantIntRange::constant(Immediate::I1(true))));
    }

    #[test]
    fn small_intersections_match_concrete_bit_patterns() {
        fn contains(range: &ConstantIntRange, value: u128) -> bool {
            let signed = unsigned_to_signed(4, value);
            range.umin <= value
                && value <= range.umax
                && range.smin <= signed
                && signed <= range.smax
        }
        for umin in 0..16 {
            for umax in umin..16 {
                let unsigned = ConstantIntRange::from_unsigned(4, umin, umax);
                for smin in -8..8 {
                    for smax in smin..8 {
                        let signed = ConstantIntRange::from_signed(4, smin, smax);
                        let intersection = unsigned.intersection(&signed);
                        for value in 0..16 {
                            assert_eq!(
                                intersection.as_ref().is_some_and(|range| contains(range, value)),
                                contains(&unsigned, value) && contains(&signed, value),
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn small_wrapping_ranges_contain_every_concrete_result() {
        for lmin in 0..16 {
            for lmax in lmin..16 {
                let lhs = ConstantIntRange::from_unsigned(4, lmin, lmax);
                for rmin in 0..16 {
                    for rmax in rmin..16 {
                        let rhs = ConstantIntRange::from_unsigned(4, rmin, rmax);
                        if let Some(result) = lhs.add_wrapping(&rhs) {
                            for lhs in lmin..=lmax {
                                for rhs in rmin..=rmax {
                                    let value = (lhs + rhs) & 15;
                                    let signed = unsigned_to_signed(4, value);
                                    assert!(result.umin <= value && value <= result.umax);
                                    assert!(result.smin <= signed && signed <= result.smax);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
