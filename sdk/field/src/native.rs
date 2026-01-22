//! Off-chain implementation of [`crate::Felt`].

use miden_core::{Felt as CoreFelt, FieldElement};

use crate::FeltImpl;

#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
/// A `Felt` represented as a felt (`miden_core::Felt`).
pub struct Felt(pub miden_core::Felt);

impl Felt {
    #[inline(always)]
    pub const fn from_u32_const(value: u32) -> Self {
        Self(CoreFelt::new(value as u64))
    }
}

impl FeltImpl for Felt {
    #[inline(always)]
    fn from_u64_unchecked(value: u64) -> Self {
        Self(CoreFelt::new(value))
    }

    #[inline(always)]
    fn from_u32(value: u32) -> Self {
        Self::from_u64_unchecked(value as u64)
    }

    #[inline(always)]
    fn as_u64(self) -> u64 {
        self.0.as_int()
    }

    #[inline(always)]
    fn is_odd(self) -> bool {
        self.as_u64() & 1 == 1
    }

    #[inline(always)]
    fn inv(self) -> Self {
        Self(self.0.inv())
    }

    #[inline(always)]
    fn pow2(self) -> Self {
        let n = self.as_u64();
        assert!(n <= 63, "pow2: exponent out of range");
        Self(CoreFelt::new(1u64 << (n as u32)))
    }

    #[inline(always)]
    fn exp(self, other: Self) -> Self {
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

impl core::fmt::Display for Felt {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.as_u64(), f)
    }
}

impl core::hash::Hash for Felt {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        core::hash::Hash::hash(&self.as_u64(), state);
    }
}
