use crate::Felt;

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
        use miden_core::{FieldElement, StarkField};
        let value = u64::try_from_str_radix(source, radix).map_err(FeltOutOfRangeError::Parse)?;
        if value > Felt::MODULUS {
            return Err(FeltOutOfRangeError::OutOfRange(value));
        }
        Ok(Felt::new(value))
    }
}
