//! A unified `Felt` for on-chain and off-chain Miden Rust code.
//!
//! This crate provides a single `Felt` type that can be used in both on-chain (Wasm) and off-chain
//! (native) Rust code:
//! - On `wasm32` targets, `Felt` is backed by a VM intrinsic-backed `f32` representation.
//! - On non-`wasm32` targets, `Felt` is backed by `miden-core`'s field element type.
//!
//! The `true-felt` feature is reserved for future work; it is not supported on `wasm32` targets in
//! this PoC.

#![no_std]
#![deny(warnings)]

/// The field modulus, `2^64 - 2^32 + 1`.
pub const MODULUS: u64 = 0xffff_ffff_0000_0001;

/// Errors returned by [`Felt::new`].
#[derive(Debug)]
pub enum FeltError {
    /// The provided value was not a valid canonical field element.
    InvalidValue,
}

#[cfg(all(target_arch = "wasm32", feature = "true-felt"))]
compile_error!("The `true-felt` feature is not supported on `wasm32` targets in this PoC");

#[cfg(target_arch = "wasm32")]
unsafe extern "C" {
    #[link_name = "intrinsics::felt::from_u64_unchecked"]
    fn extern_from_u64_unchecked(value: u64) -> Felt;

    #[link_name = "intrinsics::felt::from_u32"]
    fn extern_from_u32(value: u32) -> Felt;

    #[link_name = "intrinsics::felt::as_u64"]
    fn extern_as_u64(felt: Felt) -> u64;

    #[link_name = "intrinsics::felt::sub"]
    fn extern_sub(a: Felt, b: Felt) -> Felt;

    #[link_name = "intrinsics::felt::mul"]
    fn extern_mul(a: Felt, b: Felt) -> Felt;

    #[link_name = "intrinsics::felt::div"]
    fn extern_div(a: Felt, b: Felt) -> Felt;

    #[link_name = "intrinsics::felt::neg"]
    fn extern_neg(a: Felt) -> Felt;

    #[link_name = "intrinsics::felt::inv"]
    fn extern_inv(a: Felt) -> Felt;

    #[link_name = "intrinsics::felt::pow2"]
    fn extern_pow2(a: Felt) -> Felt;

    #[link_name = "intrinsics::felt::exp"]
    fn extern_exp(a: Felt, b: Felt) -> Felt;

    #[link_name = "intrinsics::felt::eq"]
    fn extern_eq(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::gt"]
    fn extern_gt(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::lt"]
    fn extern_lt(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::ge"]
    fn extern_ge(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::le"]
    fn extern_le(a: Felt, b: Felt) -> i32;

    #[link_name = "intrinsics::felt::is_odd"]
    fn extern_is_odd(a: Felt) -> i32;

    #[link_name = "intrinsics::felt::assert"]
    fn extern_assert(a: Felt);

    #[link_name = "intrinsics::felt::assertz"]
    fn extern_assertz(a: Felt);

    #[link_name = "intrinsics::felt::assert_eq"]
    fn extern_assert_eq(a: Felt, b: Felt);

    #[link_name = "intrinsics::felt::add"]
    fn extern_add(a: Felt, b: Felt) -> Felt;
}

#[cfg(target_arch = "wasm32")]
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
/// A field element represented as an intrinsic-backed `f32` on `wasm32` targets.
pub struct Felt {
    /// The underlying representation.
    pub inner: f32,
}

#[cfg(target_arch = "wasm32")]
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
        if value > Self::M {
            Err(FeltError::InvalidValue)
        } else {
            Ok(Self::from_u64_unchecked(value))
        }
    }

    /// Returns the canonical `u64` value of this field element.
    #[inline(always)]
    pub fn as_u64(self) -> u64 {
        unsafe { extern_as_u64(self) }
    }

    /// Returns true if this field element is odd.
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

#[cfg(target_arch = "wasm32")]
impl From<Felt> for u64 {
    fn from(felt: Felt) -> u64 {
        felt.as_u64()
    }
}

#[cfg(target_arch = "wasm32")]
impl From<u32> for Felt {
    fn from(value: u32) -> Self {
        Self {
            inner: f32::from_bits(value),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl From<u16> for Felt {
    fn from(value: u16) -> Self {
        Self {
            inner: f32::from_bits(value as u32),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl From<u8> for Felt {
    fn from(value: u8) -> Self {
        Self {
            inner: f32::from_bits(value as u32),
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_pointer_width = "32"))]
impl From<usize> for Felt {
    fn from(value: usize) -> Self {
        Self {
            inner: f32::from_bits(value as u32),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::Add for Felt {
    type Output = Self;

    #[inline(always)]
    fn add(self, other: Self) -> Self {
        unsafe { extern_add(self, other) }
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::AddAssign for Felt {
    #[inline(always)]
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::Sub for Felt {
    type Output = Self;

    #[inline(always)]
    fn sub(self, other: Self) -> Self {
        unsafe { extern_sub(self, other) }
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::SubAssign for Felt {
    #[inline(always)]
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::Mul for Felt {
    type Output = Self;

    #[inline(always)]
    fn mul(self, other: Self) -> Self {
        unsafe { extern_mul(self, other) }
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::MulAssign for Felt {
    #[inline(always)]
    fn mul_assign(&mut self, other: Self) {
        *self = *self * other;
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::Div for Felt {
    type Output = Self;

    #[inline(always)]
    fn div(self, other: Self) -> Self {
        unsafe { extern_div(self, other) }
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::DivAssign for Felt {
    #[inline(always)]
    fn div_assign(&mut self, other: Self) {
        *self = *self / other;
    }
}

#[cfg(target_arch = "wasm32")]
impl core::ops::Neg for Felt {
    type Output = Self;

    #[inline(always)]
    fn neg(self) -> Self {
        unsafe { extern_neg(self) }
    }
}

#[cfg(target_arch = "wasm32")]
impl PartialEq for Felt {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        unsafe { extern_eq(*self, *other) == 1 }
    }
}

#[cfg(target_arch = "wasm32")]
impl Eq for Felt {}

#[cfg(target_arch = "wasm32")]
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

#[cfg(target_arch = "wasm32")]
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

/// Fails if `a` != 1.
#[cfg(target_arch = "wasm32")]
#[inline(always)]
pub fn assert(a: Felt) {
    unsafe { extern_assert(a) }
}

/// Fails if `a` != 0.
#[cfg(target_arch = "wasm32")]
#[inline(always)]
pub fn assertz(a: Felt) {
    unsafe { extern_assertz(a) }
}

/// Fails if `a` != `b`.
#[cfg(target_arch = "wasm32")]
#[inline(always)]
pub fn assert_eq(a: Felt, b: Felt) {
    unsafe { extern_assert_eq(a, b) }
}

#[cfg(not(target_arch = "wasm32"))]
use miden_core::{Felt as CoreFelt, FieldElement};

#[cfg(not(target_arch = "wasm32"))]
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
/// A field element represented as `miden_core::Felt` on non-`wasm32` targets.
pub struct Felt(pub CoreFelt);

#[cfg(not(target_arch = "wasm32"))]
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
        if value > Self::M {
            Err(FeltError::InvalidValue)
        } else {
            Ok(Self::from_u64_unchecked(value))
        }
    }

    /// Returns the canonical `u64` value of this field element.
    #[inline(always)]
    pub fn as_u64(self) -> u64 {
        self.0.as_int()
    }

    /// Returns true if this field element is odd.
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

#[cfg(not(target_arch = "wasm32"))]
impl From<CoreFelt> for Felt {
    fn from(value: CoreFelt) -> Self {
        Self(value)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<Felt> for CoreFelt {
    fn from(value: Felt) -> Self {
        value.0
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<Felt> for u64 {
    fn from(felt: Felt) -> u64 {
        felt.as_u64()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<u32> for Felt {
    fn from(value: u32) -> Self {
        Self::from_u32(value)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<u16> for Felt {
    fn from(value: u16) -> Self {
        Self::from_u64_unchecked(value as u64)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<u8> for Felt {
    fn from(value: u8) -> Self {
        Self::from_u64_unchecked(value as u64)
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "32"))]
impl From<usize> for Felt {
    fn from(value: usize) -> Self {
        Self::from_u64_unchecked(value as u64)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::Add for Felt {
    type Output = Self;

    #[inline(always)]
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::AddAssign for Felt {
    #[inline(always)]
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::Sub for Felt {
    type Output = Self;

    #[inline(always)]
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::SubAssign for Felt {
    #[inline(always)]
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::Mul for Felt {
    type Output = Self;

    #[inline(always)]
    fn mul(self, other: Self) -> Self {
        Self(self.0 * other.0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::MulAssign for Felt {
    #[inline(always)]
    fn mul_assign(&mut self, other: Self) {
        *self = *self * other;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::Div for Felt {
    type Output = Self;

    #[inline(always)]
    fn div(self, other: Self) -> Self {
        Self(self.0 / other.0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::DivAssign for Felt {
    #[inline(always)]
    fn div_assign(&mut self, other: Self) {
        *self = *self / other;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl core::ops::Neg for Felt {
    type Output = Self;

    #[inline(always)]
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl PartialEq for Felt {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.as_u64() == other.as_u64()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Eq for Felt {}

#[cfg(not(target_arch = "wasm32"))]
impl PartialOrd for Felt {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Ord for Felt {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_u64().cmp(&other.as_u64())
    }
}

/// Fails if `a` != 1.
#[cfg(not(target_arch = "wasm32"))]
#[inline(always)]
pub fn assert(a: Felt) {
    if a != Felt::from_u64_unchecked(1) {
        panic!("assert: expected 1");
    }
}

/// Fails if `a` != 0.
#[cfg(not(target_arch = "wasm32"))]
#[inline(always)]
pub fn assertz(a: Felt) {
    if a != Felt::from_u64_unchecked(0) {
        panic!("assertz: expected 0");
    }
}

/// Fails if `a` != `b`.
#[cfg(not(target_arch = "wasm32"))]
#[inline(always)]
pub fn assert_eq(a: Felt, b: Felt) {
    if a != b {
        panic!("assert_eq: values differ");
    }
}
