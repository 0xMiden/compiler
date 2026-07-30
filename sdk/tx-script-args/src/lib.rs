//! Typed transaction-script argument transport via the `TX_SCRIPT_ARGS` word.
//!
//! The transaction kernel hands a transaction script a single [`Word`] of arguments. This crate
//! defines how typed argument values travel through that word: short, statically sized encodings
//! are packed into the word itself, while longer or variable-length encodings are committed to by
//! hash and passed through the advice provider.
//!
//! The crate is shared between on-chain and off-chain code: [`ScriptArgs::encode`] is pure and
//! runs anywhere, while [`ScriptArgs::decode`]'s advice-provider access is only compiled for
//! Miden VM targets — off-chain builds do not depend on the on-chain SDK bindings at all.

#![no_std]
#![deny(warnings)]

extern crate alloc;

use alloc::vec::Vec;

use miden_field::{Felt, Word};
use miden_field_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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

/// Transaction-script arguments transported through the `TX_SCRIPT_ARGS` word.
///
/// Every type that implements both [`FromFeltRepr`] and [`ToFeltRepr`] is a `ScriptArgs`
/// automatically. The felt-repr encoding selects one of two transport modes at compile time:
///
/// - **Word mode** ([`FIXED_LEN`](Self::FIXED_LEN) of at most 4 felts): the encoding is packed
///   directly into the args word; the unused felts are zero, and [`decode`](Self::decode) asserts
///   they are.
/// - **Commitment mode** (longer or variable-length encodings): the args word is the Poseidon2
///   hash of the zero-padded encoding. [`decode`](Self::decode) fetches the preimage from the
///   advice provider and the hash is verified in-VM, so the host cannot substitute values.
///
/// The mode is a compile-time property of the argument type, so the host-side encoder and the
/// guest-side decoder always agree on the transport mode. The word carries no field layout —
/// the encoding and decoding type definitions themselves must match.
pub trait ScriptArgs: Sized {
    /// Total encoded length in felts, when statically known.
    const FIXED_LEN: Option<usize>;

    /// Decodes the arguments from the `TX_SCRIPT_ARGS` word in the transaction-script guest.
    ///
    /// Panics (failing the transaction) when the encoding is malformed: a decode error, non-zero
    /// padding, or a commitment preimage that is not a whole number of words. Commitment-mode
    /// decoding requires the Miden VM; word-mode decoding is pure.
    fn decode(arg: Word) -> Self;

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

/// Decodes a value from `reader`, aborting on malformed input.
#[inline(always)]
fn decode_or_panic<T: FromFeltRepr>(reader: &mut FeltReader<'_>) -> T {
    match T::from_felt_repr(reader) {
        Ok(value) => value,
        // Panic messages are unobservable in-VM (the generated guest panic handler traps without
        // formatting), so don't pay code size for the message machinery there.
        #[cfg(all(target_family = "wasm", miden))]
        Err(_) => core::arch::wasm32::unreachable(),
        #[cfg(not(all(target_family = "wasm", miden)))]
        Err(err) => panic!("failed to decode tx script args: {err}"),
    }
}

/// Aborts decoding: an immediate trap in-VM — where panic messages are unobservable — and a panic
/// with the message elsewhere.
#[inline(always)]
fn fail(message: &str) -> ! {
    #[cfg(all(target_family = "wasm", miden))]
    {
        let _ = message;
        core::arch::wasm32::unreachable()
    }
    #[cfg(not(all(target_family = "wasm", miden)))]
    panic!("{message}")
}

/// Asserts a padding felt is zero: a native VM assert on Miden targets, a plain assert elsewhere.
#[inline(always)]
fn assert_zero_felt(felt: Felt) {
    #[cfg(all(target_family = "wasm", miden))]
    miden_stdlib_sys::assertz(felt);
    #[cfg(not(all(target_family = "wasm", miden)))]
    assert!(felt == Felt::ZERO, "expected zero padding felt in tx script args");
}

/// Asserts that every felt remaining in `reader` is zero.
#[inline(always)]
fn assert_zero_padding(reader: &mut FeltReader<'_>) {
    while reader.remaining() > 0 {
        assert_zero_felt(reader.read().expect("padding felt must be readable"));
    }
}

/// Fetches and decodes commitment-mode arguments from the advice provider.
///
/// The args word commits to the encoding: `adv_load_preimage` verifies the fetched preimage's
/// hash against it in-VM.
#[cfg(all(target_family = "wasm", miden))]
fn decode_commitment<T: FromFeltRepr>(arg: Word) -> T {
    use miden_stdlib_sys::{adv_load_preimage, assert_eq, intrinsics::advice::adv_push_mapvaln};

    let num_felts = adv_push_mapvaln(arg).as_canonical_u64();
    assert_eq(Felt::new(num_felts % WORD_FELTS as u64).unwrap(), Felt::ZERO);
    let num_words = Felt::new(num_felts / WORD_FELTS as u64).unwrap();
    let preimage = adv_load_preimage(num_words, arg);
    decode_preimage(&preimage)
}

/// Decodes a value from a commitment-mode preimage, enforcing the canonical zero padding.
///
/// Target-independent so the canonicality rules are unit-testable natively; only the VM decode
/// path calls it outside tests.
#[cfg_attr(not(all(target_family = "wasm", miden)), allow(dead_code))]
#[inline(always)]
fn decode_preimage<T: FromFeltRepr>(preimage: &[Felt]) -> T {
    let mut reader = FeltReader::new(preimage);
    let value = decode_or_panic(&mut reader);
    // Only the zero felts padding the encoding to a whole number of words may remain.
    if reader.remaining() >= WORD_FELTS {
        fail("trailing data after tx script args");
    }
    assert_zero_padding(&mut reader);
    value
}

/// Commitment-mode decoding requires the advice provider, which only exists on the Miden VM.
#[cfg(not(all(target_family = "wasm", miden)))]
fn decode_commitment<T: FromFeltRepr>(_arg: Word) -> T {
    unimplemented!("commitment-mode tx script args can only be decoded on the Miden VM")
}

impl<T: FromFeltRepr + ToFeltRepr> ScriptArgs for T {
    const FIXED_LEN: Option<usize> = <T as FromFeltRepr>::FIXED_LEN;

    fn decode(arg: Word) -> Self {
        // A const-evaluated branch guarantees the dead transport path is never codegenned.
        if const { is_word_mode(Self::FIXED_LEN) } {
            let felts = [arg[0], arg[1], arg[2], arg[3]];
            let mut reader = FeltReader::new(&felts);
            let value = decode_or_panic(&mut reader);
            assert_zero_padding(&mut reader);
            value
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

        assert_eq!(<Felt as ScriptArgs>::decode(word), value);
    }

    /// A full word of arguments is transported as-is.
    #[test]
    fn word_args_are_transported_verbatim() {
        let word = Word::new([felt(1), felt(2), felt(3), felt(4)]);
        let EncodedScriptArgs::Word(encoded) = word.encode() else {
            panic!("expected word mode for a word");
        };

        assert_eq!(encoded, word);
        assert_eq!(<Word as ScriptArgs>::decode(encoded), word);
    }

    /// Non-zero felts in the unused part of the args word fail the decode.
    #[test]
    #[should_panic(expected = "zero padding")]
    fn word_mode_decode_rejects_nonzero_padding() {
        let word = Word::new([felt(7), felt(0), felt(0), felt(1)]);
        let _ = <Felt as ScriptArgs>::decode(word);
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

    /// A commitment preimage with only zero padding decodes.
    #[test]
    fn decode_preimage_accepts_canonical_padding() {
        let decoded: Vec<Felt> = decode_preimage(&[felt(2), felt(5), felt(6), felt(0)]);
        assert_eq!(decoded, vec![felt(5), felt(6)]);
    }

    /// A non-zero felt in the padding region fails the decode.
    #[test]
    #[should_panic(expected = "zero padding")]
    fn decode_preimage_rejects_nonzero_padding() {
        let _: Vec<Felt> = decode_preimage(&[felt(2), felt(5), felt(6), felt(9)]);
    }

    /// A whole extra word beyond the encoding fails the decode, even when it is all zeros.
    #[test]
    #[should_panic(expected = "trailing data")]
    fn decode_preimage_rejects_extra_word() {
        let _: Vec<Felt> = decode_preimage(&[
            felt(2),
            felt(5),
            felt(6),
            felt(0),
            felt(0),
            felt(0),
            felt(0),
            felt(0),
        ]);
    }
}
