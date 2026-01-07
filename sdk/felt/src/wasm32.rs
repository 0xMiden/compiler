//! On-chain implementation of [`crate::Felt`].

use crate::{Felt, FeltError, MODULUS};

unsafe extern "C" {
    #[link_name = "intrinsics::felt::from_u64_unchecked"]
    pub(crate) fn extern_from_u64_unchecked(value: u64) -> Felt;

    #[link_name = "intrinsics::felt::from_u32"]
    pub(crate) fn extern_from_u32(value: u32) -> Felt;

    #[link_name = "intrinsics::felt::as_u64"]
    pub(crate) fn extern_as_u64(felt: Felt) -> u64;

    #[link_name = "intrinsics::felt::sub"]
    pub(crate) fn extern_sub(a: Felt, b: Felt) -> Felt;

    #[link_name = "intrinsics::felt::mul"]
    pub(crate) fn extern_mul(a: Felt, b: Felt) -> Felt;

    #[link_name = "intrinsics::felt::div"]
    pub(crate) fn extern_div(a: Felt, b: Felt) -> Felt;

    #[link_name = "intrinsics::felt::neg"]
    pub(crate) fn extern_neg(a: Felt) -> Felt;

    #[link_name = "intrinsics::felt::inv"]
    pub(crate) fn extern_inv(a: Felt) -> Felt;

    #[link_name = "intrinsics::felt::pow2"]
    pub(crate) fn extern_pow2(a: Felt) -> Felt;

    #[link_name = "intrinsics::felt::exp"]
    pub(crate) fn extern_exp(a: Felt, b: Felt) -> Felt;

    #[link_name = "intrinsics::felt::eq"]
    pub(crate) fn extern_eq(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::gt"]
    pub(crate) fn extern_gt(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::lt"]
    pub(crate) fn extern_lt(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::ge"]
    pub(crate) fn extern_ge(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::le"]
    pub(crate) fn extern_le(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::is_odd"]
    pub(crate) fn extern_is_odd(a: Felt) -> i32;

    #[link_name = "intrinsics::felt::add"]
    pub(crate) fn extern_add(a: Felt, b: Felt) -> Felt;
}

impl Felt {
    /// Field modulus = 2^64 - 2^32 + 1.
    pub const M: u64 = MODULUS;

    /// Creates a `Felt` from `value` without range checks.
    #[inline(always)]
    pub fn from_u64_unchecked(value: u64) -> Self {
        unsafe { extern_from_u64_unchecked(value) }
    }

    /// Creates a `Felt` from a `u32` value.
    #[inline(always)]
    pub fn from_u32(value: u32) -> Self {
        unsafe { extern_from_u32(value) }
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
        unsafe { extern_as_u64(self) }
    }

    /// Returns true if this felt is odd.
    #[inline(always)]
    pub fn is_odd(self) -> bool {
        unsafe { extern_is_odd(self) != 0 }
    }

    /// Returns `self^-1`. Fails if `self = 0`.
    #[inline(always)]
    pub fn inv(self) -> Self {
        unsafe { extern_inv(self) }
    }

    /// Returns `2^self`. Fails if `self > 63`.
    #[inline(always)]
    pub fn pow2(self) -> Self {
        unsafe { extern_pow2(self) }
    }

    /// Returns `self^other`.
    #[inline(always)]
    pub fn exp(self, other: Self) -> Self {
        unsafe { extern_exp(self, other) }
    }
}

impl From<Felt> for u64 {
    fn from(felt: Felt) -> u64 {
        felt.as_u64()
    }
}

impl From<u32> for Felt {
    fn from(value: u32) -> Self {
        Self {
            inner: f32::from_bits(value),
        }
    }
}

impl From<u16> for Felt {
    fn from(value: u16) -> Self {
        Self {
            inner: f32::from_bits(value as u32),
        }
    }
}

impl From<u8> for Felt {
    fn from(value: u8) -> Self {
        Self {
            inner: f32::from_bits(value as u32),
        }
    }
}

#[cfg(target_pointer_width = "32")]
impl From<usize> for Felt {
    fn from(value: usize) -> Self {
        Self {
            inner: f32::from_bits(value as u32),
        }
    }
}

impl core::ops::Add for Felt {
    type Output = Self;

    #[inline(always)]
    fn add(self, other: Self) -> Self {
        unsafe { extern_add(self, other) }
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
        unsafe { extern_sub(self, other) }
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
        unsafe { extern_mul(self, other) }
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
        unsafe { extern_div(self, other) }
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
        unsafe { extern_neg(self) }
    }
}

impl PartialEq for Felt {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        unsafe { extern_eq(*self, *other) == 1 }
    }
}

impl Eq for Felt {}

impl PartialOrd for Felt {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }

    #[inline(always)]
    fn gt(&self, other: &Self) -> bool {
        unsafe { extern_gt(*self, *other) != 0 }
    }

    #[inline(always)]
    fn ge(&self, other: &Self) -> bool {
        unsafe { extern_ge(*self, *other) != 0 }
    }

    #[inline(always)]
    fn lt(&self, other: &Self) -> bool {
        unsafe { extern_lt(*other, *self) != 0 }
    }

    #[inline(always)]
    fn le(&self, other: &Self) -> bool {
        unsafe { extern_le(*other, *self) != 0 }
    }
}

impl Ord for Felt {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        if self.lt(other) {
            core::cmp::Ordering::Less
        } else if self.gt(other) {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        }
    }
}

// Note: public `assert` helpers live in `sdk/felt/src/lib.rs` to preserve their stable paths in
// emitted WASM and expected-file tests.
