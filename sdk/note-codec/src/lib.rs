//! Author-side support for typed note storage codecs.
//!
//! Codec components exchange field elements as canonical `u64` values. This crate checks every
//! integer before it enters [`Felt`], so a component cannot introduce a reduced or ambiguous field
//! representation.

#![deny(missing_docs)]

pub use miden_field_repr::*;
#[doc(hidden)]
pub use miden_note_codec_macros::from_wit_text;
pub use miden_note_codec_macros::{export_codecs, from_package, from_project, note_codec};
pub use miden_protocol::{account, asset};

/// The WIT document implemented by generated note codec components.
pub const NOTE_CODEC_WIT: &str = include_str!("../wit/note-codec.wit");

/// Parses, displays, and validates one author-defined note storage type.
pub trait AuthorTypeCodec: Sized {
    /// Parses text into a typed value.
    fn parse(value: &str) -> Result<Self, String>;

    /// Displays a typed value.
    fn display(&self) -> String;

    /// Validates semantic constraints that are not part of the felt layout.
    fn validate(&self) -> Result<(), String>;
}

/// Converts a field element to its canonical component-boundary integer.
pub fn felt_to_u64(value: Felt) -> u64 {
    value.as_canonical_u64()
}

/// Converts a component-boundary integer to a canonical field element.
pub fn felt_from_u64(value: u64) -> Result<Felt, String> {
    Felt::new(value).map_err(|error| format!("invalid component felt `{value}`: {error}"))
}

/// Converts field elements to canonical component-boundary integers.
pub fn felts_to_u64(values: &[Felt]) -> Vec<u64> {
    values.iter().copied().map(felt_to_u64).collect()
}

/// Converts component-boundary integers to canonical field elements.
pub fn felts_from_u64(values: &[u64]) -> Result<Vec<Felt>, String> {
    values
        .iter()
        .copied()
        .enumerate()
        .map(|(index, value)| {
            felt_from_u64(value)
                .map_err(|error| format!("component felt at index {index} is invalid: {error}"))
        })
        .collect()
}

/// Encodes a native felt-repr value for the component boundary.
pub fn encode_felt_repr(value: &impl ToFeltRepr) -> Vec<u64> {
    felts_to_u64(&value.to_felt_repr())
}

/// Decodes one complete native felt-repr value from the component boundary.
pub fn decode_felt_repr<T: FromFeltRepr>(values: &[u64]) -> Result<T, String> {
    let felts = felts_from_u64(values)?;
    let mut reader = FeltReader::new(&felts);
    let value = T::from_felt_repr(&mut reader)
        .map_err(|error| format!("invalid felt representation: {error}"))?;
    reader
        .ensure_eof()
        .map_err(|error| format!("invalid felt representation: {error}"))?;
    Ok(value)
}

/// Support used by generated host-profile types.
#[doc(hidden)]
pub mod __private {
    pub use miden_field;
    pub use miden_field_repr;
    pub use miden_protocol;
    pub use wit_bindgen;

    /// Lightweight error support required by shared generated type helpers.
    pub mod miden_note_schema {
        use core::fmt;

        /// A generated type conversion error.
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct Error(String);

        impl Error {
            /// Creates a generated type conversion error.
            pub fn new(message: impl Into<String>) -> Self {
                Self(message.into())
            }
        }

        impl fmt::Display for Error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl std::error::Error for Error {}

        /// A generated type conversion result.
        pub type Result<T> = core::result::Result<T, Error>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample author type used to test the native boundary glue.
    #[derive(Clone, Debug, Eq, PartialEq, FromFeltRepr, ToFeltRepr)]
    struct Ratio {
        numerator: u64,
        denominator: u64,
    }

    impl AuthorTypeCodec for Ratio {
        fn parse(value: &str) -> Result<Self, String> {
            let (numerator, denominator) = value
                .split_once('/')
                .ok_or_else(|| "a ratio must use `numerator/denominator`".to_owned())?;
            Ok(Self {
                numerator: numerator.parse::<u64>().map_err(|error| error.to_string())?,
                denominator: denominator.parse::<u64>().map_err(|error| error.to_string())?,
            })
        }

        fn display(&self) -> String {
            format!("{}/{}", self.numerator, self.denominator)
        }

        fn validate(&self) -> Result<(), String> {
            if self.denominator == 0 {
                Err("the denominator must not be zero".to_owned())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn author_trait_values_round_trip_through_boundary_glue() {
        let ratio = Ratio::parse("3/2").unwrap();
        ratio.validate().unwrap();
        let encoded = encode_felt_repr(&ratio);
        assert_eq!(encoded, [3, 0, 2, 0]);
        let decoded = decode_felt_repr::<Ratio>(&encoded).unwrap();
        assert_eq!(decoded, ratio);
        assert_eq!(decoded.display(), "3/2");
    }

    #[test]
    fn boundary_rejects_noncanonical_field_integers() {
        let maximum = Felt::ORDER - 1;
        assert_eq!(felt_to_u64(felt_from_u64(maximum).unwrap()), maximum);
        assert!(felt_from_u64(Felt::ORDER).unwrap_err().contains("exceeds the felt modulus"));
        assert!(felt_from_u64(u64::MAX).unwrap_err().contains("exceeds the felt modulus"));
        assert!(felts_from_u64(&[0, Felt::ORDER]).unwrap_err().contains("index 1"));
    }
}
