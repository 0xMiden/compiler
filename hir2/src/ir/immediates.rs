use core::{
    fmt,
    hash::{Hash, Hasher},
};

pub use miden_core::{Felt, FieldElement, StarkField};

use crate::{formatter::PrettyPrint, Type};

#[derive(Debug, Copy, Clone)]
pub enum Immediate {
    I1(bool),
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    U128(u128),
    I128(i128),
    F64(f64),
    Felt(Felt),
}
impl Immediate {
    pub fn ty(&self) -> Type {
        match self {
            Self::I1(_) => Type::I1,
            Self::U8(_) => Type::U8,
            Self::I8(_) => Type::I8,
            Self::U16(_) => Type::U16,
            Self::I16(_) => Type::I16,
            Self::U32(_) => Type::U32,
            Self::I32(_) => Type::I32,
            Self::U64(_) => Type::U64,
            Self::I64(_) => Type::I64,
            Self::U128(_) => Type::U128,
            Self::I128(_) => Type::I128,
            Self::F64(_) => Type::F64,
            Self::Felt(_) => Type::Felt,
        }
    }

    /// Returns true if this immediate is a non-negative value
    pub fn is_non_negative(&self) -> bool {
        match self {
            Self::I1(i) => *i,
            Self::I8(i) => *i > 0,
            Self::U8(i) => *i > 0,
            Self::I16(i) => *i > 0,
            Self::U16(i) => *i > 0,
            Self::I32(i) => *i > 0,
            Self::U32(i) => *i > 0,
            Self::I64(i) => *i > 0,
            Self::U64(i) => *i > 0,
            Self::U128(i) => *i > 0,
            Self::I128(i) => *i > 0,
            Self::F64(f) => f.is_sign_positive(),
            Self::Felt(_) => true,
        }
    }

    /// Returns true if this immediate can represent negative values
    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            Self::I8(_) | Self::I16(_) | Self::I32(_) | Self::I64(_) | Self::I128(_) | Self::F64(_)
        )
    }

    /// Returns true if this immediate can only represent non-negative values
    pub fn is_unsigned(&self) -> bool {
        matches!(
            self,
            Self::I1(_)
                | Self::U8(_)
                | Self::U16(_)
                | Self::U32(_)
                | Self::U64(_)
                | Self::U128(_)
                | Self::Felt(_)
        )
    }

    /// Returns true if this immediate is an odd integer, otherwise false
    ///
    /// If the immediate is not an integer, returns `None`
    pub fn is_odd(&self) -> Option<bool> {
        match self {
            Self::I1(b) => Some(*b),
            Self::U8(i) => Some(*i % 2 == 0),
            Self::I8(i) => Some(*i % 2 == 0),
            Self::U16(i) => Some(*i % 2 == 0),
            Self::I16(i) => Some(*i % 2 == 0),
            Self::U32(i) => Some(*i % 2 == 0),
            Self::I32(i) => Some(*i % 2 == 0),
            Self::U64(i) => Some(*i % 2 == 0),
            Self::I64(i) => Some(*i % 2 == 0),
            Self::Felt(i) => Some(i.as_int() % 2 == 0),
            Self::U128(i) => Some(*i % 2 == 0),
            Self::I128(i) => Some(*i % 2 == 0),
            Self::F64(_) => None,
        }
    }

    /// Returns true if this immediate is a non-zero integer, otherwise false
    ///
    /// If the immediate is not an integer, returns `None`
    pub fn as_bool(self) -> Option<bool> {
        match self {
            Self::I1(b) => Some(b),
            Self::U8(i) => Some(i != 0),
            Self::I8(i) => Some(i != 0),
            Self::U16(i) => Some(i != 0),
            Self::I16(i) => Some(i != 0),
            Self::U32(i) => Some(i != 0),
            Self::I32(i) => Some(i != 0),
            Self::U64(i) => Some(i != 0),
            Self::I64(i) => Some(i != 0),
            Self::Felt(i) => Some(i.as_int() != 0),
            Self::U128(i) => Some(i != 0),
            Self::I128(i) => Some(i != 0),
            Self::F64(_) => None,
        }
    }

    /// Attempts to convert this value to a u32
    pub fn as_u32(self) -> Option<u32> {
        match self {
            Self::I1(b) => Some(b as u32),
            Self::U8(b) => Some(b as u32),
            Self::I8(b) if b >= 0 => Some(b as u32),
            Self::I8(_) => None,
            Self::U16(b) => Some(b as u32),
            Self::I16(b) if b >= 0 => Some(b as u32),
            Self::I16(_) => None,
            Self::U32(b) => Some(b),
            Self::I32(b) if b >= 0 => Some(b as u32),
            Self::I32(_) => None,
            Self::U64(b) => u32::try_from(b).ok(),
            Self::I64(b) if b >= 0 => u32::try_from(b as u64).ok(),
            Self::I64(_) => None,
            Self::Felt(i) => u32::try_from(i.as_int()).ok(),
            Self::U128(b) if b <= (u32::MAX as u64 as u128) => Some(b as u32),
            Self::U128(_) => None,
            Self::I128(b) if b >= 0 && b <= (u32::MAX as u64 as i128) => Some(b as u32),
            Self::I128(_) => None,
            Self::F64(f) => FloatToInt::<u32>::to_int(f).ok(),
        }
    }

    /// Attempts to convert this value to i32
    pub fn as_i32(self) -> Option<i32> {
        match self {
            Self::I1(b) => Some(b as i32),
            Self::U8(i) => Some(i as i32),
            Self::I8(i) => Some(i as i32),
            Self::U16(i) => Some(i as i32),
            Self::I16(i) => Some(i as i32),
            Self::U32(i) => i.try_into().ok(),
            Self::I32(i) => Some(i),
            Self::U64(i) => i.try_into().ok(),
            Self::I64(i) => i.try_into().ok(),
            Self::Felt(i) => i.as_int().try_into().ok(),
            Self::U128(i) if i <= (i32::MAX as u32 as u128) => Some(i as u32 as i32),
            Self::U128(_) => None,
            Self::I128(i) if i >= (i32::MIN as i128) && i <= (i32::MAX as i128) => Some(i as i32),
            Self::I128(_) => None,
            Self::F64(f) => FloatToInt::<i32>::to_int(f).ok(),
        }
    }

    /// Attempts to convert this value to a field element
    pub fn as_felt(self) -> Option<Felt> {
        match self {
            Self::I1(b) => Some(Felt::new(b as u64)),
            Self::U8(b) => Some(Felt::new(b as u64)),
            Self::I8(b) => u64::try_from(b).ok().map(Felt::new),
            Self::U16(b) => Some(Felt::new(b as u64)),
            Self::I16(b) => u64::try_from(b).ok().map(Felt::new),
            Self::U32(b) => Some(Felt::new(b as u64)),
            Self::I32(b) => u64::try_from(b).ok().map(Felt::new),
            Self::U64(b) => Some(Felt::new(b)),
            Self::I64(b) => u64::try_from(b).ok().map(Felt::new),
            Self::Felt(i) => Some(i),
            Self::U128(b) => u64::try_from(b).ok().map(Felt::new),
            Self::I128(b) => u64::try_from(b).ok().map(Felt::new),
            Self::F64(f) => FloatToInt::<Felt>::to_int(f).ok(),
        }
    }

    /// Attempts to convert this value to u64
    pub fn as_u64(self) -> Option<u64> {
        match self {
            Self::I1(b) => Some(b as u64),
            Self::U8(i) => Some(i as u64),
            Self::I8(i) if i >= 0 => Some(i as u64),
            Self::I8(_) => None,
            Self::U16(i) => Some(i as u64),
            Self::I16(i) if i >= 0 => Some(i as u16 as u64),
            Self::I16(_) => None,
            Self::U32(i) => Some(i as u64),
            Self::I32(i) if i >= 0 => Some(i as u32 as u64),
            Self::I32(_) => None,
            Self::U64(i) => Some(i),
            Self::I64(i) if i >= 0 => Some(i as u64),
            Self::I64(_) => None,
            Self::Felt(i) => Some(i.as_int()),
            Self::U128(i) => (i).try_into().ok(),
            Self::I128(i) if i >= 0 => (i).try_into().ok(),
            Self::I128(_) => None,
            Self::F64(f) => FloatToInt::<u64>::to_int(f).ok(),
        }
    }

    /// Attempts to convert this value to i64
    pub fn as_i64(self) -> Option<i64> {
        match self {
            Self::I1(b) => Some(b as i64),
            Self::U8(i) => Some(i as i64),
            Self::I8(i) => Some(i as i64),
            Self::U16(i) => Some(i as i64),
            Self::I16(i) => Some(i as i64),
            Self::U32(i) => Some(i as i64),
            Self::I32(i) => Some(i as i64),
            Self::U64(i) => (i).try_into().ok(),
            Self::I64(i) => Some(i),
            Self::Felt(i) => i.as_int().try_into().ok(),
            Self::U128(i) if i <= i64::MAX as u128 => Some(i as u64 as i64),
            Self::U128(_) => None,
            Self::I128(i) => (i).try_into().ok(),
            Self::F64(f) => FloatToInt::<i64>::to_int(f).ok(),
        }
    }

    /// Attempts to convert this value to u128
    pub fn as_u128(self) -> Option<u128> {
        match self {
            Self::I1(b) => Some(b as u128),
            Self::U8(i) => Some(i as u128),
            Self::I8(i) if i >= 0 => Some(i as u128),
            Self::I8(_) => None,
            Self::U16(i) => Some(i as u128),
            Self::I16(i) if i >= 0 => Some(i as u16 as u128),
            Self::I16(_) => None,
            Self::U32(i) => Some(i as u128),
            Self::I32(i) if i >= 0 => Some(i as u32 as u128),
            Self::I32(_) => None,
            Self::U64(i) => Some(i as u128),
            Self::I64(i) if i >= 0 => Some(i as u128),
            Self::I64(_) => None,
            Self::Felt(i) => Some(i.as_int() as u128),
            Self::U128(i) => Some(i),
            Self::I128(i) if i >= 0 => (i).try_into().ok(),
            Self::I128(_) => None,
            Self::F64(f) => FloatToInt::<u128>::to_int(f).ok(),
        }
    }

    /// Attempts to convert this value to i128
    pub fn as_i128(self) -> Option<i128> {
        match self {
            Self::I1(b) => Some(b as i128),
            Self::U8(i) => Some(i as i128),
            Self::I8(i) => Some(i as i128),
            Self::U16(i) => Some(i as i128),
            Self::I16(i) => Some(i as i128),
            Self::U32(i) => Some(i as i128),
            Self::I32(i) => Some(i as i128),
            Self::U64(i) => Some(i as i128),
            Self::I64(i) => Some(i as i128),
            Self::Felt(i) => Some(i.as_int() as i128),
            Self::U128(i) if i <= i128::MAX as u128 => Some(i as i128),
            Self::U128(_) => None,
            Self::I128(i) => Some(i),
            Self::F64(f) => FloatToInt::<i128>::to_int(f).ok(),
        }
    }
}
impl fmt::Display for Immediate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::I1(i) => write!(f, "{}", i),
            Self::U8(i) => write!(f, "{}", i),
            Self::I8(i) => write!(f, "{}", i),
            Self::U16(i) => write!(f, "{}", i),
            Self::I16(i) => write!(f, "{}", i),
            Self::U32(i) => write!(f, "{}", i),
            Self::I32(i) => write!(f, "{}", i),
            Self::U64(i) => write!(f, "{}", i),
            Self::I64(i) => write!(f, "{}", i),
            Self::U128(i) => write!(f, "{}", i),
            Self::I128(i) => write!(f, "{}", i),
            Self::F64(n) => write!(f, "{}", n),
            Self::Felt(i) => write!(f, "{}", i),
        }
    }
}
impl PrettyPrint for Immediate {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        display(self)
    }
}
impl Hash for Immediate {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let d = std::mem::discriminant(self);
        d.hash(state);
        match self {
            Self::I1(i) => i.hash(state),
            Self::U8(i) => i.hash(state),
            Self::I8(i) => i.hash(state),
            Self::U16(i) => i.hash(state),
            Self::I16(i) => i.hash(state),
            Self::U32(i) => i.hash(state),
            Self::I32(i) => i.hash(state),
            Self::U64(i) => i.hash(state),
            Self::I64(i) => i.hash(state),
            Self::U128(i) => i.hash(state),
            Self::I128(i) => i.hash(state),
            Self::F64(f) => {
                let bytes = f.to_be_bytes();
                bytes.hash(state)
            }
            Self::Felt(i) => i.as_int().hash(state),
        }
    }
}
impl Eq for Immediate {}
impl PartialEq for Immediate {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::I8(x), Self::I8(y)) => x == y,
            (Self::U16(x), Self::U16(y)) => x == y,
            (Self::I16(x), Self::I16(y)) => x == y,
            (Self::U32(x), Self::U32(y)) => x == y,
            (Self::I32(x), Self::I32(y)) => x == y,
            (Self::U64(x), Self::U64(y)) => x == y,
            (Self::I64(x), Self::I64(y)) => x == y,
            (Self::U128(x), Self::U128(y)) => x == y,
            (Self::I128(x), Self::I128(y)) => x == y,
            (Self::F64(x), Self::F64(y)) => x == y,
            (Self::Felt(x), Self::Felt(y)) => x == y,
            _ => false,
        }
    }
}
impl PartialEq<isize> for Immediate {
    fn eq(&self, other: &isize) -> bool {
        let y = *other;
        match *self {
            Self::I1(x) => x == (y == 1),
            Self::U8(_) if y < 0 => false,
            Self::U8(x) => x as isize == y,
            Self::I8(x) => x as isize == y,
            Self::U16(_) if y < 0 => false,
            Self::U16(x) => x as isize == y,
            Self::I16(x) => x as isize == y,
            Self::U32(_) if y < 0 => false,
            Self::U32(x) => x as isize == y,
            Self::I32(x) => x as isize == y,
            Self::U64(_) if y < 0 => false,
            Self::U64(x) => x == y as i64 as u64,
            Self::I64(x) => x == y as i64,
            Self::U128(_) if y < 0 => false,
            Self::U128(x) => x == y as i128 as u128,
            Self::I128(x) => x == y as i128,
            Self::F64(_) => false,
            Self::Felt(_) if y < 0 => false,
            Self::Felt(x) => x.as_int() == y as i64 as u64,
        }
    }
}
impl PartialOrd for Immediate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Immediate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        match (self, other) {
            // Floats require special treatment
            (Self::F64(x), Self::F64(y)) => x.total_cmp(y),
            // Here we're attempting to compare against any integer immediate,
            // so we must attempt to convert the float to the largest possible
            // integer representation, i128, and then promote the integer immediate
            // to i128 for comparison
            //
            // If the float is not an integer value, truncate it and compare, then
            // adjust the result to account for the truncation
            (Self::F64(x), y) => {
                let y = y
                    .as_i128()
                    .expect("expected rhs to be an integer capable of fitting in an i128");
                if let Ok(x) = FloatToInt::<i128>::to_int(*x) {
                    x.cmp(&y)
                } else {
                    let is_positive = x.is_sign_positive();
                    if let Ok(x) = FloatToInt::<i128>::to_int((*x).trunc()) {
                        // Edge case for equality: the float must be bigger due to truncation
                        match x.cmp(&y) {
                            Ordering::Equal if is_positive => Ordering::Greater,
                            Ordering::Equal => Ordering::Less,
                            o => o,
                        }
                    } else {
                        // The float is larger than i128 can represent, the sign tells us in what
                        // direction
                        if is_positive {
                            Ordering::Greater
                        } else {
                            Ordering::Less
                        }
                    }
                }
            }
            (x, y @ Self::F64(_)) => y.cmp(x).reverse(),
            // u128 immediates require separate treatment
            (Self::U128(x), Self::U128(y)) => x.cmp(y),
            (Self::U128(x), y) => {
                let y = y.as_u128().expect("expected rhs to be an integer in the range of u128");
                x.cmp(&y)
            }
            (x, Self::U128(y)) => {
                let x = x.as_u128().expect("expected lhs to be an integer in the range of u128");
                x.cmp(y)
            }
            // i128 immediates require separate treatment
            (Self::I128(x), Self::I128(y)) => x.cmp(y),
            // We're only comparing against values here which are u64, i64, or smaller than 64-bits
            (Self::I128(x), y) => {
                let y = y.as_i128().expect("expected rhs to be an integer smaller than i128");
                x.cmp(&y)
            }
            (x, Self::I128(y)) => {
                let x = x.as_i128().expect("expected lhs to be an integer smaller than i128");
                x.cmp(y)
            }
            // u64 immediates may not fit in an i64
            (Self::U64(x), Self::U64(y)) => x.cmp(y),
            // We're only comparing against values here which are i64, or smaller than 64-bits
            (Self::U64(x), y) => {
                let y =
                    y.as_i64().expect("expected rhs to be an integer capable of fitting in an i64")
                        as u64;
                x.cmp(&y)
            }
            (x, Self::U64(y)) => {
                let x =
                    x.as_i64().expect("expected lhs to be an integer capable of fitting in an i64")
                        as u64;
                x.cmp(y)
            }
            // All immediates at this point are i64 or smaller
            (x, y) => {
                let x =
                    x.as_i64().expect("expected lhs to be an integer capable of fitting in an i64");
                let y =
                    y.as_i64().expect("expected rhs to be an integer capable of fitting in an i64");
                x.cmp(&y)
            }
        }
    }
}
impl From<Immediate> for Type {
    #[inline]
    fn from(imm: Immediate) -> Self {
        imm.ty()
    }
}
impl From<&Immediate> for Type {
    #[inline(always)]
    fn from(imm: &Immediate) -> Self {
        imm.ty()
    }
}
impl From<bool> for Immediate {
    #[inline(always)]
    fn from(value: bool) -> Self {
        Self::I1(value)
    }
}
impl From<i8> for Immediate {
    #[inline(always)]
    fn from(value: i8) -> Self {
        Self::I8(value)
    }
}
impl From<u8> for Immediate {
    #[inline(always)]
    fn from(value: u8) -> Self {
        Self::U8(value)
    }
}
impl From<i16> for Immediate {
    #[inline(always)]
    fn from(value: i16) -> Self {
        Self::I16(value)
    }
}
impl From<u16> for Immediate {
    #[inline(always)]
    fn from(value: u16) -> Self {
        Self::U16(value)
    }
}
impl From<i32> for Immediate {
    #[inline(always)]
    fn from(value: i32) -> Self {
        Self::I32(value)
    }
}
impl From<u32> for Immediate {
    #[inline(always)]
    fn from(value: u32) -> Self {
        Self::U32(value)
    }
}
impl From<i64> for Immediate {
    #[inline(always)]
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}
impl From<u64> for Immediate {
    #[inline(always)]
    fn from(value: u64) -> Self {
        Self::U64(value)
    }
}
impl From<u128> for Immediate {
    #[inline(always)]
    fn from(value: u128) -> Self {
        Self::U128(value)
    }
}
impl From<i128> for Immediate {
    #[inline(always)]
    fn from(value: i128) -> Self {
        Self::I128(value)
    }
}
impl From<f64> for Immediate {
    #[inline(always)]
    fn from(value: f64) -> Self {
        Self::F64(value)
    }
}
impl From<char> for Immediate {
    #[inline(always)]
    fn from(value: char) -> Self {
        Self::I32(value as u32 as i32)
    }
}

trait FloatToInt<T: Sized + Copy>: Sized {
    const ZERO: T;

    fn upper_bound() -> Self;
    fn lower_bound() -> Self;
    fn to_int(self) -> Result<T, ()>;
    unsafe fn to_int_unchecked(self) -> T;
}
impl FloatToInt<i8> for f64 {
    const ZERO: i8 = 0;

    fn upper_bound() -> Self {
        f64::from(i8::MAX) + 1.0
    }

    fn lower_bound() -> Self {
        f64::from(i8::MIN) - 1.0
    }

    fn to_int(self) -> Result<i8, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> i8 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<u8> for f64 {
    const ZERO: u8 = 0;

    fn upper_bound() -> Self {
        f64::from(u8::MAX) + 1.0
    }

    fn lower_bound() -> Self {
        0.0
    }

    fn to_int(self) -> Result<u8, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> u8 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<i16> for f64 {
    const ZERO: i16 = 0;

    fn upper_bound() -> Self {
        f64::from(i16::MAX) + 1.0
    }

    fn lower_bound() -> Self {
        f64::from(i16::MIN) - 1.0
    }

    fn to_int(self) -> Result<i16, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> i16 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<u16> for f64 {
    const ZERO: u16 = 0;

    fn upper_bound() -> Self {
        f64::from(u16::MAX) + 1.0
    }

    fn lower_bound() -> Self {
        0.0
    }

    fn to_int(self) -> Result<u16, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> u16 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<i32> for f64 {
    const ZERO: i32 = 0;

    fn upper_bound() -> Self {
        f64::from(i32::MAX) + 1.0
    }

    fn lower_bound() -> Self {
        f64::from(i32::MIN) - 1.0
    }

    fn to_int(self) -> Result<i32, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> i32 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<u32> for f64 {
    const ZERO: u32 = 0;

    fn upper_bound() -> Self {
        f64::from(u32::MAX) + 1.0
    }

    fn lower_bound() -> Self {
        0.0
    }

    fn to_int(self) -> Result<u32, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> u32 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<i64> for f64 {
    const ZERO: i64 = 0;

    fn upper_bound() -> Self {
        63.0f64.exp2()
    }

    fn lower_bound() -> Self {
        (63.0f64.exp2() * -1.0) - 1.0
    }

    fn to_int(self) -> Result<i64, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> i64 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<u64> for f64 {
    const ZERO: u64 = 0;

    fn upper_bound() -> Self {
        64.0f64.exp2()
    }

    fn lower_bound() -> Self {
        0.0
    }

    fn to_int(self) -> Result<u64, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> u64 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<Felt> for f64 {
    const ZERO: Felt = Felt::ZERO;

    fn upper_bound() -> Self {
        64.0f64.exp2() - 32.0f64.exp2() + 1.0
    }

    fn lower_bound() -> Self {
        0.0
    }

    fn to_int(self) -> Result<Felt, ()> {
        float_to_int(self).map(Felt::new)
    }

    unsafe fn to_int_unchecked(self) -> Felt {
        Felt::new(f64::to_int_unchecked::<u64>(self))
    }
}
impl FloatToInt<u128> for f64 {
    const ZERO: u128 = 0;

    fn upper_bound() -> Self {
        128.0f64.exp2()
    }

    fn lower_bound() -> Self {
        0.0
    }

    fn to_int(self) -> Result<u128, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> u128 {
        f64::to_int_unchecked(self)
    }
}
impl FloatToInt<i128> for f64 {
    const ZERO: i128 = 0;

    fn upper_bound() -> Self {
        f64::from(i128::BITS - 1).exp2()
    }

    fn lower_bound() -> Self {
        (f64::from(i128::BITS - 1) * -1.0).exp2() - 1.0
    }

    fn to_int(self) -> Result<i128, ()> {
        float_to_int(self)
    }

    unsafe fn to_int_unchecked(self) -> i128 {
        f64::to_int_unchecked(self)
    }
}

fn float_to_int<I>(f: f64) -> Result<I, ()>
where
    I: Copy,
    f64: FloatToInt<I>,
{
    use std::num::FpCategory;
    match f.classify() {
        FpCategory::Nan | FpCategory::Infinite | FpCategory::Subnormal => Err(()),
        FpCategory::Zero => Ok(<f64 as FloatToInt<I>>::ZERO),
        FpCategory::Normal => {
            if f == f.trunc()
                && f > <f64 as FloatToInt<I>>::lower_bound()
                && f < <f64 as FloatToInt<I>>::upper_bound()
            {
                // SAFETY: We know that x must be integral, and within the bounds of its type
                Ok(unsafe { <f64 as FloatToInt<I>>::to_int_unchecked(f) })
            } else {
                Err(())
            }
        }
    }
}
