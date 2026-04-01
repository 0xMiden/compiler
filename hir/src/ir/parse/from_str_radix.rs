use crate::{Felt, Immediate};

pub trait FromStrRadix: Sized {
    type Error: core::error::Error;

    fn try_from_str_radix(source: &str, radix: u32) -> Result<Self, Self::Error>;
}

macro_rules! from_str_radix_impl {
    ($($t:ty),*) => {
        $(
            impl FromStrRadix for $t {
                type Error = core::num::ParseIntError;

                #[inline]
                fn try_from_str_radix(source: &str, radix: u32) -> Result<Self, Self::Error> {
                    <$t>::from_str_radix(source, radix)
                }
            }
        )*
    }
}

from_str_radix_impl!(u8, i8, u16, i16, u32, i32, u64, i64, u128, i128, usize, isize);

#[doc(hidden)]
#[derive(Debug, thiserror::Error)]
pub enum FeltOutOfRangeError {
    #[error(transparent)]
    Parse(#[from] core::num::ParseIntError),
    #[error("invalid felt: {0} is larger than the field modulus")]
    OutOfRange(u64),
}

impl FromStrRadix for Felt {
    type Error = FeltOutOfRangeError;

    fn try_from_str_radix(source: &str, radix: u32) -> Result<Self, Self::Error> {
        let value = u64::try_from_str_radix(source, radix).map_err(FeltOutOfRangeError::Parse)?;
        if value > Felt::ORDER {
            return Err(FeltOutOfRangeError::OutOfRange(value));
        }
        Ok(Felt::new(value))
    }
}

impl FromStrRadix for Immediate {
    type Error = core::num::ParseIntError;

    fn try_from_str_radix(source: &str, radix: u32) -> Result<Self, Self::Error> {
        if source.starts_with('-') {
            let n = i128::try_from_str_radix(source, radix)?;
            match n {
                n if n > (i64::MAX as i128) || n < (i64::MIN as i128) => Ok(Immediate::I128(n)),
                n if n > (i32::MAX as i128) || n < (i32::MIN as i128) => {
                    Ok(Immediate::I64(n as i64))
                }
                n if n > (i16::MAX as i128) || n < (i16::MIN as i128) => {
                    Ok(Immediate::I32(n as i32))
                }
                n if n > (i8::MAX as i128) || n < (i8::MIN as i128) => Ok(Immediate::I16(n as i16)),
                n @ (0i128..=1i128) => Ok(Immediate::I8(n as i8)),
                n => Ok(Immediate::I1(n == 1i128)),
            }
        } else {
            let n = u128::try_from_str_radix(source, radix)?;
            match n {
                n if n > (u64::MAX as u128) => Ok(Immediate::U128(n)),
                n if n > (u32::MAX as u128) => Ok(Immediate::U64(n as u64)),
                n if n > (u16::MAX as u128) => Ok(Immediate::U32(n as u32)),
                n if n > (u8::MAX as u128) => Ok(Immediate::U16(n as u16)),
                n if n > 1u128 => Ok(Immediate::U8(n as u8)),
                n => Ok(Immediate::I1(n == 1u128)),
            }
        }
    }
}
