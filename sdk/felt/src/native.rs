//! Off-chain implementation of [`crate::Felt`].

use miden_core::{Felt as CoreFelt, FieldElement};

use crate::{Felt, FeltError, MODULUS};

impl Felt {
    /// Field modulus = 2^64 - 2^32 + 1.
    pub const M: u64 = MODULUS;

    /// Creates a `Felt` from `value` without range checks.
    #[inline(always)]
    pub fn from_u64_unchecked(value: u64) -> Self {
        Self(CoreFelt::new(value))
    }

    /// Creates a `Felt` from a `u32` value.
    #[inline(always)]
    pub fn from_u32(value: u32) -> Self {
        Self::from_u64_unchecked(value as u64)
    }

    /// Creates a `Felt` from `value`, returning an error if it is out of range.
    #[inline(always)]
    pub fn new(value: u64) -> Result<Self, FeltError> {
        if value >= Self::M {
            Err(FeltError::InvalidValue)
        } else {
            Ok(Self::from_u64_unchecked(value))
        }
    }

    /// Returns the canonical `u64` value of this felt.
    #[inline(always)]
    pub fn as_u64(self) -> u64 {
        self.0.as_int()
    }

    /// Returns true if this felt is odd.
    #[inline(always)]
    pub fn is_odd(self) -> bool {
        self.as_u64() & 1 == 1
    }

    /// Returns `self^-1`. Fails if `self = 0`.
    #[inline(always)]
    pub fn inv(self) -> Self {
        Self(self.0.inv())
    }

    /// Returns `2^self`. Fails if `self > 63`.
    #[inline(always)]
    pub fn pow2(self) -> Self {
        let n = self.as_u64();
        assert!(n <= 63, "pow2: exponent out of range");
        Self(CoreFelt::new(1u64 << (n as u32)))
    }

    /// Returns `self^other`.
    #[inline(always)]
    pub fn exp(self, other: Self) -> Self {
        Self(self.0.exp(other.as_u64()))
    }
}

impl From<CoreFelt> for Felt {
    fn from(value: CoreFelt) -> Self {
        Self(value)
    }
}

impl From<Felt> for CoreFelt {
    fn from(value: Felt) -> Self {
        value.0
    }
}

impl From<Felt> for u64 {
    fn from(felt: Felt) -> u64 {
        felt.as_u64()
    }
}

impl From<u32> for Felt {
    fn from(value: u32) -> Self {
        Self::from_u32(value)
    }
}

impl From<u16> for Felt {
    fn from(value: u16) -> Self {
        Self::from_u64_unchecked(value as u64)
    }
}

impl From<u8> for Felt {
    fn from(value: u8) -> Self {
        Self::from_u64_unchecked(value as u64)
    }
}

#[cfg(target_pointer_width = "32")]
impl From<usize> for Felt {
    fn from(value: usize) -> Self {
        Self::from_u64_unchecked(value as u64)
    }
}

impl core::ops::Add for Felt {
    type Output = Self;

    #[inline(always)]
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl core::ops::AddAssign for Felt {
    #[inline(always)]
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl core::ops::Sub for Felt {
    type Output = Self;

    #[inline(always)]
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl core::ops::SubAssign for Felt {
    #[inline(always)]
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl core::ops::Mul for Felt {
    type Output = Self;

    #[inline(always)]
    fn mul(self, other: Self) -> Self {
        Self(self.0 * other.0)
    }
}

impl core::ops::MulAssign for Felt {
    #[inline(always)]
    fn mul_assign(&mut self, other: Self) {
        *self = *self * other;
    }
}

impl core::ops::Div for Felt {
    type Output = Self;

    #[inline(always)]
    fn div(self, other: Self) -> Self {
        Self(self.0 / other.0)
    }
}

impl core::ops::DivAssign for Felt {
    #[inline(always)]
    fn div_assign(&mut self, other: Self) {
        *self = *self / other;
    }
}

impl core::ops::Neg for Felt {
    type Output = Self;

    #[inline(always)]
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl PartialEq for Felt {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.as_u64() == other.as_u64()
    }
}

impl Eq for Felt {}

impl PartialOrd for Felt {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Felt {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_u64().cmp(&other.as_u64())
    }
}

// Note: Felt assertion intrinsics live in `miden-stdlib-sys`.
