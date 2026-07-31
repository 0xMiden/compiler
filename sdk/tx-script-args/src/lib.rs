//! Typed transaction-script argument transport via the `TX_SCRIPT_ARGS` word.
//!
//! The transaction kernel hands a transaction script a single [`Word`] of arguments. This crate
//! defines how typed argument values travel through that word: short, statically sized encodings
//! are packed into the word itself, while longer or variable-length encodings are committed to by
//! hash and passed through the advice provider.
//!
//! The crate is shared between on-chain and off-chain code: [`ScriptArgs::encode`],
//! [`decode_preimage`], and word-mode [`ScriptArgs::decode`] are pure and run anywhere, while
//! the advice-provider transport sits behind the `miden-vm-guest` feature (enabled by the
//! `miden` SDK crate) — no host build, native or wasm, depends on the on-chain SDK bindings.

#![no_std]
#![deny(warnings)]

extern crate alloc;

use alloc::vec::Vec;

use miden_field::{Felt, Word};
use miden_field_repr::{FeltReader, FeltReprError, FromFeltRepr, ToFeltRepr};

/// Number of felts packed into a [`Word`].
const WORD_FELTS: usize = Word::NUM_ELEMENTS;

/// Encoded transaction-script arguments produced by [`ScriptArgs::encode`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodedScriptArgs {
    /// The encoding statically fits the `TX_SCRIPT_ARGS` word and is packed into it directly,
    /// zero-padded.
    Word(Word),
    /// The type uses commitment mode (a longer or variable-length encoding): these felts
    /// (zero-padded to a whole number of words) are the advice-map preimage of the args word.
    /// The host hashes them with `Poseidon2::hash_elements` — the Miden VM's native hash — to
    /// obtain the args word, and registers `(args_word, felts)` in the advice map.
    Preimage(Vec<Felt>),
}

/// Failure decoding transaction-script arguments from the `TX_SCRIPT_ARGS` word.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScriptArgsError {
    /// A value failed to decode from its felt representation.
    Decode(FeltReprError),
    /// A felt in the zero-padding region of the encoding was not zero.
    NonZeroPadding,
    /// A whole word or more of data remained after decoding the value (commitment mode).
    TrailingData,
    /// The advice value's length was not a whole number of words (commitment mode).
    NonWordMultipleLength,
    /// Commitment-mode decoding was attempted without the Miden VM's advice provider
    /// (i.e. off-chain). Word-mode decoding and [`decode_preimage`] work everywhere.
    AdviceProviderUnavailable,
}

impl From<FeltReprError> for ScriptArgsError {
    fn from(err: FeltReprError) -> Self {
        Self::Decode(err)
    }
}

impl core::fmt::Display for ScriptArgsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Decode(err) => write!(f, "failed to decode tx script args: {err}"),
            Self::NonZeroPadding => f.write_str("non-zero padding felt in tx script args"),
            Self::TrailingData => f.write_str("trailing data after tx script args"),
            Self::NonWordMultipleLength => {
                f.write_str("tx script args advice value is not a whole number of words")
            }
            Self::AdviceProviderUnavailable => {
                f.write_str("commitment-mode tx script args can only be decoded on the Miden VM")
            }
        }
    }
}

/// Convenience alias for results returned by script-args decoding.
pub type ScriptArgsResult<T> = core::result::Result<T, ScriptArgsError>;

mod sealed {
    /// Seals [`ScriptArgs`](super::ScriptArgs) to its blanket implementation.
    pub trait Sealed {}
    impl<T: super::FromFeltRepr + super::ToFeltRepr> Sealed for T {}
}

/// Transaction-script arguments transported through the `TX_SCRIPT_ARGS` word.
///
/// Every type that implements both [`FromFeltRepr`] and [`ToFeltRepr`] is a `ScriptArgs`
/// automatically. The felt-repr encoding selects one of two transport modes at compile time:
///
/// - **Word mode** ([`FIXED_LEN`](Self::FIXED_LEN) of at most 4 felts): the encoding is packed
///   directly into the args word; the unused felts are zero, and [`decode`](Self::decode) rejects
///   anything else.
/// - **Commitment mode** (longer or variable-length encodings): the args word is the Poseidon2
///   hash of the zero-padded encoding. [`decode`](Self::decode) fetches the preimage from the
///   advice provider and the hash is verified in-VM, so the host cannot substitute values.
///
/// The mode is a compile-time property of the argument type, so within one type definition the
/// encoder and decoder always agree on the transport mode. The word carries no field layout —
/// host-side mirrors of a guest type must reproduce its felt-repr wire sequence (see the
/// migration guide for how to pin that).
///
/// The trait is sealed: the blanket `FromFeltRepr + ToFeltRepr` implementation is the only one,
/// which is what makes the documented transport guarantees hold for every implementor.
pub trait ScriptArgs: Sized + sealed::Sealed {
    /// Total encoded length in felts, when statically known.
    const FIXED_LEN: Option<usize>;

    /// Decodes the arguments from the `TX_SCRIPT_ARGS` word.
    ///
    /// Returns an error when the encoding is malformed: a felt-repr decode failure, non-zero
    /// padding, or a non-canonical commitment preimage. The `#[tx_script]`-generated entrypoint
    /// wrapper panics on the error, failing the transaction. Commitment-mode decoding requires
    /// the Miden VM; word-mode decoding is pure.
    fn decode(arg: Word) -> ScriptArgsResult<Self>;

    /// Encodes the arguments for transaction construction on the host.
    fn encode(&self) -> EncodedScriptArgs;
}

/// Returns whether a statically known encoded length fits the args word directly.
const fn is_word_mode(fixed_len: Option<usize>) -> bool {
    match fixed_len {
        Some(len) => len <= WORD_FELTS,
        None => false,
    }
}

/// Ensures every felt remaining in `reader` is zero.
#[inline(always)]
fn check_zero_padding(reader: &mut FeltReader<'_>) -> ScriptArgsResult<()> {
    while reader.remaining() > 0 {
        if reader.read()? != Felt::ZERO {
            return Err(ScriptArgsError::NonZeroPadding);
        }
    }
    Ok(())
}

/// Fetches and decodes commitment-mode arguments from the advice provider.
///
/// The args word commits to the encoding: `adv_load_preimage` verifies the fetched preimage's
/// hash against it in-VM.
#[cfg(all(target_family = "wasm", miden, feature = "miden-vm-guest"))]
#[inline(always)]
fn decode_commitment<T: FromFeltRepr>(arg: Word) -> ScriptArgsResult<T> {
    use miden_stdlib_sys::{adv_load_preimage, intrinsics::advice::adv_push_mapvaln};

    let num_felts = adv_push_mapvaln(arg).as_canonical_u64();
    if num_felts % WORD_FELTS as u64 != 0 {
        return Err(ScriptArgsError::NonWordMultipleLength);
    }
    let num_words = Felt::new(num_felts / WORD_FELTS as u64).unwrap();
    let preimage = adv_load_preimage(num_words, arg);
    decode_preimage(&preimage)
}

/// Decodes a value from a commitment-mode preimage, enforcing the canonical zero padding.
///
/// This is the pure half of commitment-mode [`ScriptArgs::decode`]: it runs anywhere, so hosts
/// can round-trip [`EncodedScriptArgs::Preimage`] bytes in tests and tooling without the VM.
#[inline(always)]
pub fn decode_preimage<T: FromFeltRepr>(preimage: &[Felt]) -> ScriptArgsResult<T> {
    let mut reader = FeltReader::new(preimage);
    let value = T::from_felt_repr(&mut reader)?;
    check_decoded_len::<T>(&reader)?;
    // Only the zero felts padding the encoding to a whole number of words may remain.
    if reader.remaining() >= WORD_FELTS {
        return Err(ScriptArgsError::TrailingData);
    }
    check_zero_padding(&mut reader)?;
    Ok(value)
}

/// Asserts a decoder consumed exactly its declared `FIXED_LEN` felts, catching manual
/// implementations whose decode disagrees with the constant (the encode side is checked by
/// [`ScriptArgs::encode`]).
#[inline(always)]
fn check_decoded_len<T: FromFeltRepr>(reader: &FeltReader<'_>) -> ScriptArgsResult<()> {
    if let Some(fixed_len) = T::FIXED_LEN {
        assert!(reader.pos() == fixed_len, "decoded length must match FIXED_LEN");
    }
    Ok(())
}

/// Commitment-mode decoding requires the advice provider, which only exists on the Miden VM.
#[cfg(not(all(target_family = "wasm", miden, feature = "miden-vm-guest")))]
fn decode_commitment<T: FromFeltRepr>(_arg: Word) -> ScriptArgsResult<T> {
    Err(ScriptArgsError::AdviceProviderUnavailable)
}

impl<T: FromFeltRepr + ToFeltRepr> ScriptArgs for T {
    const FIXED_LEN: Option<usize> = <T as FromFeltRepr>::FIXED_LEN;

    // Inlined so the caller's error match fuses with the decode and the `Result` never has to
    // materialize in memory on the happy path.
    #[inline(always)]
    fn decode(arg: Word) -> ScriptArgsResult<Self> {
        // A const-evaluated branch guarantees the dead transport path is never codegenned.
        if const { is_word_mode(Self::FIXED_LEN) } {
            let felts = [arg[0], arg[1], arg[2], arg[3]];
            let mut reader = FeltReader::new(&felts);
            let value = Self::from_felt_repr(&mut reader)?;
            check_decoded_len::<Self>(&reader)?;
            check_zero_padding(&mut reader)?;
            Ok(value)
        } else {
            decode_commitment(arg)
        }
    }

    fn encode(&self) -> EncodedScriptArgs {
        let mut felts = self.to_felt_repr();
        // Validate before selecting the transport: a wrong manual `FIXED_LEN` would otherwise
        // silently truncate the args word or mis-route the encoding.
        if let Some(fixed_len) = Self::FIXED_LEN {
            assert_eq!(felts.len(), fixed_len, "encoding length must match FIXED_LEN");
        }
        if const { is_word_mode(Self::FIXED_LEN) } {
            while felts.len() < WORD_FELTS {
                felts.push(Felt::ZERO);
            }
            EncodedScriptArgs::Word(Word::new([felts[0], felts[1], felts[2], felts[3]]))
        } else {
            // Zero-pad to a whole number of words; the padding is part of the hashed preimage.
            while !felts.len().is_multiple_of(WORD_FELTS) {
                felts.push(Felt::ZERO);
            }
            EncodedScriptArgs::Preimage(felts)
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    fn felt(value: u64) -> Felt {
        Felt::new(value).unwrap()
    }

    /// Word mode packs the encoding into the args word and zero-pads the unused felts.
    #[test]
    fn word_mode_encode_pads_with_zeros() {
        let EncodedScriptArgs::Word(word) = felt(7).encode() else {
            panic!("expected word mode for a single felt");
        };

        assert_eq!(word, Word::new([felt(7), felt(0), felt(0), felt(0)]));
    }

    /// Word mode decodes straight from the args word, without the advice provider.
    #[test]
    fn word_mode_roundtrip() {
        let value = felt(7);
        let EncodedScriptArgs::Word(word) = value.encode() else {
            panic!("expected word mode for a single felt");
        };

        assert_eq!(<Felt as ScriptArgs>::decode(word), Ok(value));
    }

    /// A full word of arguments is transported as-is.
    #[test]
    fn word_args_are_transported_verbatim() {
        let word = Word::new([felt(1), felt(2), felt(3), felt(4)]);
        let EncodedScriptArgs::Word(encoded) = word.encode() else {
            panic!("expected word mode for a word");
        };

        assert_eq!(encoded, word);
        assert_eq!(<Word as ScriptArgs>::decode(encoded), Ok(word));
    }

    /// Non-zero felts in the unused part of the args word fail the decode.
    #[test]
    fn word_mode_decode_rejects_nonzero_padding() {
        let word = Word::new([felt(7), felt(0), felt(0), felt(1)]);

        assert_eq!(<Felt as ScriptArgs>::decode(word), Err(ScriptArgsError::NonZeroPadding));
    }

    /// A word-mode decode error surfaces the underlying felt-repr failure.
    #[test]
    fn word_mode_decode_surfaces_felt_repr_errors() {
        let word = Word::new([felt(2), felt(0), felt(0), felt(0)]);

        assert_eq!(
            <bool as ScriptArgs>::decode(word),
            Err(ScriptArgsError::Decode(FeltReprError::InvalidBool {
                pos: 0,
                len: 4,
                value: 2
            }))
        );
    }

    /// Commitment mode zero-pads the preimage to a whole number of words.
    #[test]
    fn commitment_mode_encode_pads_to_word_multiple() {
        let values = vec![felt(5), felt(6)];
        let EncodedScriptArgs::Preimage(felts) = values.encode() else {
            panic!("expected commitment mode for a variable-length encoding");
        };

        // Length prefix, two elements, one felt of padding.
        assert_eq!(felts, vec![felt(2), felt(5), felt(6), felt(0)]);
    }

    /// A manual implementation whose `FIXED_LEN` disagrees with its actual encoding.
    struct LyingFixedLen;

    impl FromFeltRepr for LyingFixedLen {
        const FIXED_LEN: Option<usize> = Some(1);

        fn from_felt_repr(reader: &mut FeltReader<'_>) -> miden_field_repr::FeltReprResult<Self> {
            reader.read()?;
            reader.read()?;
            Ok(Self)
        }
    }

    impl ToFeltRepr for LyingFixedLen {
        fn write_felt_repr(&self, writer: &mut miden_field_repr::FeltWriter<'_>) {
            writer.write(felt(1));
            writer.write(felt(2));
        }
    }

    /// A wrong manual `FIXED_LEN` must fail loudly instead of truncating the args word.
    #[test]
    #[should_panic(expected = "must match FIXED_LEN")]
    fn encode_rejects_wrong_manual_fixed_len() {
        let _ = LyingFixedLen.encode();
    }

    /// A decoder that consumes fewer felts than its declared `FIXED_LEN` must fail loudly
    /// instead of mistaking argument felts for padding.
    #[test]
    #[should_panic(expected = "decoded length must match FIXED_LEN")]
    fn decode_rejects_wrong_manual_fixed_len() {
        /// Declares two felts but consumes one.
        struct LyingDecoder;

        impl FromFeltRepr for LyingDecoder {
            const FIXED_LEN: Option<usize> = Some(2);

            fn from_felt_repr(
                reader: &mut FeltReader<'_>,
            ) -> miden_field_repr::FeltReprResult<Self> {
                reader.read()?;
                Ok(Self)
            }
        }

        impl ToFeltRepr for LyingDecoder {
            fn write_felt_repr(&self, writer: &mut miden_field_repr::FeltWriter<'_>) {
                writer.write(felt(1));
                writer.write(felt(2));
            }
        }

        let _ = LyingDecoder::decode(Word::new([felt(1), felt(2), felt(0), felt(0)]));
    }

    /// Off-VM, commitment-mode decoding reports the missing advice provider as an error.
    #[test]
    fn commitment_mode_decode_reports_missing_advice_provider() {
        let word = Word::new([felt(1), felt(2), felt(3), felt(4)]);

        assert_eq!(
            <Vec<Felt> as ScriptArgs>::decode(word),
            Err(ScriptArgsError::AdviceProviderUnavailable)
        );
    }

    /// A commitment preimage with only zero padding decodes.
    #[test]
    fn decode_preimage_accepts_canonical_padding() {
        let decoded: ScriptArgsResult<Vec<Felt>> =
            decode_preimage(&[felt(2), felt(5), felt(6), felt(0)]);

        assert_eq!(decoded, Ok(vec![felt(5), felt(6)]));
    }

    /// A non-zero felt in the padding region fails the decode.
    #[test]
    fn decode_preimage_rejects_nonzero_padding() {
        let decoded: ScriptArgsResult<Vec<Felt>> =
            decode_preimage(&[felt(2), felt(5), felt(6), felt(9)]);

        assert_eq!(decoded, Err(ScriptArgsError::NonZeroPadding));
    }

    /// A whole extra word beyond the encoding fails the decode, even when it is all zeros.
    #[test]
    fn decode_preimage_rejects_extra_word() {
        let decoded: ScriptArgsResult<Vec<Felt>> = decode_preimage(&[
            felt(2),
            felt(5),
            felt(6),
            felt(0),
            felt(0),
            felt(0),
            felt(0),
            felt(0),
        ]);

        assert_eq!(decoded, Err(ScriptArgsError::TrailingData));
    }
}
