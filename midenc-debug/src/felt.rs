use std::collections::VecDeque;

use miden_core::{FieldElement, StarkField, Word};
use miden_processor::Felt as RawFelt;
use midenc_hir::{smallvec, SmallVec};
use proptest::{
    arbitrary::Arbitrary,
    strategy::{BoxedStrategy, Strategy},
};
use serde::Deserialize;

pub trait ToMidenRepr {
    /// Convert this type into its raw byte representation
    ///
    /// The order of bytes in the resulting vector should be little-endian, i.e. the least
    /// significant bytes come first.
    fn to_bytes(&self) -> SmallVec<[u8; 16]>;
    /// Convert this type into one or more field elements, where the order of the elements is such
    /// that the byte representation of `self` is in little-endian order, i.e. the least significant
    /// bytes come first.
    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        let bytes = self.to_bytes();
        let num_felts = bytes.len().next_multiple_of(4) / 4;
        let mut felts = SmallVec::<[RawFelt; 4]>::with_capacity(num_felts);
        let mut chunks = bytes.into_iter().array_chunks::<4>();
        for chunk in chunks.by_ref() {
            felts.push(RawFelt::new(u32::from_ne_bytes(chunk) as u64));
        }
        if let Some(remainder) = chunks.into_remainder().filter(|r| r.len() > 0) {
            if remainder.len() > 0 {
                let mut chunk = [0u8; 4];
                for (i, byte) in remainder.enumerate() {
                    chunk[i] = byte;
                }
                felts.push(RawFelt::new(u32::from_ne_bytes(chunk) as u64));
            }
        }
        felts
    }
    /// Convert this type into one or more words, zero-padding as needed, such that:
    ///
    /// * The field elements within each word is in little-endian order, i.e. the least significant
    ///   bytes of come first.
    /// * Each word, if pushed on the operand stack element-by-element, would leave the element
    ///   with the most significant bytes on top of the stack (including padding)
    fn to_words(&self) -> SmallVec<[Word; 1]> {
        let felts = self.to_felts();
        let num_words = felts.len().next_multiple_of(4) / 4;
        let mut words = SmallVec::<[Word; 1]>::with_capacity(num_words);
        let mut chunks = felts.into_iter().array_chunks::<4>();
        for mut word in chunks.by_ref() {
            word.reverse();
            words.push(Word::new(word));
        }
        if let Some(remainder) = chunks.into_remainder().filter(|r| r.len() > 0) {
            if remainder.len() > 0 {
                let mut word = [RawFelt::ZERO; 4];
                for (i, felt) in remainder.enumerate() {
                    word[i] = felt;
                }
                word.reverse();
                words.push(Word::new(word));
            }
        }
        words
    }

    /// Push this value on the given operand stack using [Self::to_felts] representation
    fn push_to_operand_stack(&self, stack: &mut Vec<RawFelt>) {
        let felts = self.to_felts();
        for felt in felts.into_iter().rev() {
            stack.push(felt);
        }
    }

    /// Push this value in its [Self::to_words] representation, on the given stack.
    ///
    /// This function is designed for encoding values that will be placed on the advice stack and
    /// copied into Miden VM memory by the compiler-emitted test harness.
    ///
    /// Returns the number of words that were pushed on the stack
    fn push_words_to_advice_stack(&self, stack: &mut Vec<RawFelt>) -> usize {
        let words = self.to_words();
        let num_words = words.len();
        for word in words.into_iter().rev() {
            for felt in word.into_iter() {
                stack.push(felt);
            }
        }
        num_words
    }
}

pub trait FromMidenRepr: Sized {
    /// Returns the size of this type as encoded by [ToMidenRepr::to_felts]
    fn size_in_felts() -> usize;
    /// Extract a value of this type from `bytes`, where:
    ///
    /// * It is assumed that bytes is always padded out to 4 byte alignment
    /// * It is assumed that the bytes are in little-endian order, as encoded by [ToMidenRepr]
    fn from_bytes(bytes: &[u8]) -> Self;
    /// Extract a value of this type as encoded in a vector of field elements, where:
    ///
    /// * The order of the field elements is little-endian, i.e. the element holding the least
    ///   significant bytes comes first.
    fn from_felts(felts: &[RawFelt]) -> Self {
        let mut bytes = SmallVec::<[u8; 16]>::with_capacity(felts.len() * 4);
        for felt in felts {
            let chunk = (felt.as_int() as u32).to_ne_bytes();
            bytes.extend(chunk);
        }
        Self::from_bytes(&bytes)
    }
    /// Extract a value of this type as encoded in a vector of words, where:
    ///
    /// * The order of the words is little-endian, i.e. the word holding the least significant
    ///   bytes comes first.
    /// * The order of the field elements in each word is in big-endian order, i.e. the element
    ///   with the most significant byte is at the start of the word, and the element with the
    ///   least significant byte is at the end of the word. This corresponds to the order in
    ///   which elements are placed on the operand stack when preparing to read or write them
    ///   from Miden's memory.
    fn from_words(words: &[Word]) -> Self {
        let mut felts = SmallVec::<[RawFelt; 4]>::with_capacity(words.len() * 4);
        for word in words {
            for felt in word.iter().copied().rev() {
                felts.push(felt);
            }
        }
        Self::from_felts(&felts)
    }

    /// Pop a value of this type from `stack` based on the canonical representation of this type
    /// on the operand stack when writing it to memory (and as read from memory).
    fn pop_from_stack(stack: &mut Vec<RawFelt>) -> Self {
        let needed = Self::size_in_felts();
        let mut felts = SmallVec::<[RawFelt; 4]>::with_capacity(needed);
        for _ in 0..needed {
            felts.push(stack.pop().unwrap());
        }
        Self::from_felts(&felts)
    }
}

impl ToMidenRepr for bool {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        smallvec![*self as u8]
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![RawFelt::new(*self as u64)]
    }

    fn push_to_operand_stack(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u64));
    }
}

impl FromMidenRepr for bool {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        match bytes[0] {
            0 => false,
            1 => true,
            n => panic!("invalid byte representation for boolean: {n:0x}"),
        }
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        match felts[0].as_int() {
            0 => false,
            1 => true,
            n => panic!("invalid byte representation for boolean: {n:0x}"),
        }
    }

    fn pop_from_stack(stack: &mut Vec<RawFelt>) -> Self {
        match stack.pop().unwrap().as_int() {
            0 => false,
            1 => true,
            n => panic!("invalid byte representation for boolean: {n:0x}"),
        }
    }
}

impl ToMidenRepr for u8 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        smallvec![*self]
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![RawFelt::new(*self as u64)]
    }

    fn push_to_operand_stack(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u64));
    }
}

impl FromMidenRepr for u8 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    #[inline(always)]
    fn from_bytes(bytes: &[u8]) -> Self {
        bytes[0]
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        felts[0].as_int() as u8
    }

    fn pop_from_stack(stack: &mut Vec<RawFelt>) -> Self {
        stack.pop().unwrap().as_int() as u8
    }
}

impl ToMidenRepr for i8 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        smallvec![*self as u8]
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![RawFelt::new(*self as u8 as u64)]
    }

    fn push_to_operand_stack(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u8 as u64));
    }
}

impl FromMidenRepr for i8 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    #[inline(always)]
    fn from_bytes(bytes: &[u8]) -> Self {
        bytes[0] as i8
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        felts[0].as_int() as u8 as i8
    }

    fn pop_from_stack(stack: &mut Vec<RawFelt>) -> Self {
        stack.pop().unwrap().as_int() as u8 as i8
    }
}

impl ToMidenRepr for u16 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(&self.to_ne_bytes())
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![RawFelt::new(*self as u64)]
    }

    fn push_to_operand_stack(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u64));
    }
}

impl FromMidenRepr for u16 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 2);
        u16::from_ne_bytes([bytes[0], bytes[1]])
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        felts[0].as_int() as u16
    }

    fn pop_from_stack(stack: &mut Vec<RawFelt>) -> Self {
        stack.pop().unwrap().as_int() as u16
    }
}

impl ToMidenRepr for i16 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(&self.to_ne_bytes())
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![RawFelt::new(*self as u16 as u64)]
    }

    fn push_to_operand_stack(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u16 as u64));
    }
}

impl FromMidenRepr for i16 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 2);
        i16::from_ne_bytes([bytes[0], bytes[1]])
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        felts[0].as_int() as u16 as i16
    }

    fn pop_from_stack(stack: &mut Vec<RawFelt>) -> Self {
        stack.pop().unwrap().as_int() as u16 as i16
    }
}

impl ToMidenRepr for u32 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(&self.to_ne_bytes())
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![RawFelt::new(*self as u64)]
    }

    fn push_to_operand_stack(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u64));
    }
}

impl FromMidenRepr for u32 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 4);
        u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        felts[0].as_int() as u32
    }

    fn pop_from_stack(stack: &mut Vec<RawFelt>) -> Self {
        stack.pop().unwrap().as_int() as u32
    }
}

impl ToMidenRepr for i32 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(&self.to_ne_bytes())
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![RawFelt::new(*self as u32 as u64)]
    }

    fn push_to_operand_stack(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u32 as u64));
    }
}

impl FromMidenRepr for i32 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 4);
        i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        felts[0].as_int() as u32 as i32
    }

    fn pop_from_stack(stack: &mut Vec<RawFelt>) -> Self {
        stack.pop().unwrap().as_int() as u32 as i32
    }
}

impl ToMidenRepr for u64 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(&self.to_be_bytes())
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        let bytes = self.to_be_bytes();
        let hi = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let lo = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        smallvec![RawFelt::new(hi as u64), RawFelt::new(lo as u64)]
    }
}

impl FromMidenRepr for u64 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        2
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 8);
        u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        assert!(felts.len() >= 2);
        let hi = (felts[0].as_int() as u32).to_be_bytes();
        let lo = (felts[1].as_int() as u32).to_be_bytes();
        u64::from_be_bytes([hi[0], hi[1], hi[2], hi[3], lo[0], lo[1], lo[2], lo[3]])
    }
}

impl ToMidenRepr for i64 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(&self.to_be_bytes())
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        (*self as u64).to_felts()
    }
}

impl FromMidenRepr for i64 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        2
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        u64::from_bytes(bytes) as i64
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        u64::from_felts(felts) as i64
    }
}

impl ToMidenRepr for u128 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(&self.to_be_bytes())
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        let bytes = self.to_be_bytes();
        let hi_h =
            RawFelt::new(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64);
        let hi_l =
            RawFelt::new(u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as u64);
        let lo_h =
            RawFelt::new(u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as u64);
        let lo_l =
            RawFelt::new(u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as u64);

        // The 64-bit limbs are little endian, (lo, hi), but the 32-bit limbs of those 64-bit
        // values are big endian, (lo_h, lo_l) and (hi_h, hi_l).
        smallvec![lo_h, lo_l, hi_h, hi_l]
    }
}

impl FromMidenRepr for u128 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        4
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 16);
        u128::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ])
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        assert!(felts.len() >= 4);
        let hi_h = (felts[0].as_int() as u32).to_be_bytes();
        let hi_l = (felts[1].as_int() as u32).to_be_bytes();
        let lo_h = (felts[2].as_int() as u32).to_be_bytes();
        let lo_l = (felts[3].as_int() as u32).to_be_bytes();
        u128::from_be_bytes([
            hi_h[0], hi_h[1], hi_h[2], hi_h[3], hi_l[0], hi_l[1], hi_l[2], hi_l[3], lo_h[0],
            lo_h[1], lo_h[2], lo_h[3], lo_l[0], lo_l[1], lo_l[2], lo_l[3],
        ])
    }
}

impl ToMidenRepr for i128 {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(&self.to_be_bytes())
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        (*self as u128).to_felts()
    }
}

impl FromMidenRepr for i128 {
    #[inline(always)]
    fn size_in_felts() -> usize {
        4
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        u128::from_bytes(bytes) as i128
    }

    fn from_felts(felts: &[RawFelt]) -> Self {
        u128::from_felts(felts) as i128
    }
}

impl ToMidenRepr for RawFelt {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        panic!("field elements have no canonical byte representation")
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![*self]
    }

    fn to_words(&self) -> SmallVec<[Word; 1]> {
        let mut word = [RawFelt::ZERO; 4];
        word[0] = *self;
        smallvec![Word::new(word)]
    }
}

impl FromMidenRepr for RawFelt {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        panic!("field elements have no canonical byte representation")
    }

    #[inline(always)]
    fn from_felts(felts: &[RawFelt]) -> Self {
        felts[0]
    }

    #[inline(always)]
    fn from_words(words: &[Word]) -> Self {
        words[0][0]
    }
}

impl ToMidenRepr for Felt {
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        panic!("field elements have no canonical byte representation")
    }

    fn to_felts(&self) -> SmallVec<[RawFelt; 4]> {
        smallvec![self.0]
    }

    fn to_words(&self) -> SmallVec<[Word; 1]> {
        let mut word = [RawFelt::ZERO; 4];
        word[0] = self.0;
        smallvec![Word::new(word)]
    }
}

impl FromMidenRepr for Felt {
    #[inline(always)]
    fn size_in_felts() -> usize {
        1
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        panic!("field elements have no canonical byte representation")
    }

    #[inline(always)]
    fn from_felts(felts: &[RawFelt]) -> Self {
        Felt(felts[0])
    }

    #[inline(always)]
    fn from_words(words: &[Word]) -> Self {
        Felt(words[0][0])
    }
}

impl<const N: usize> ToMidenRepr for [u8; N] {
    #[inline]
    fn to_bytes(&self) -> SmallVec<[u8; 16]> {
        SmallVec::from_slice(self)
    }
}

impl<const N: usize> FromMidenRepr for [u8; N] {
    #[inline(always)]
    fn size_in_felts() -> usize {
        N.next_multiple_of(4) / 4
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= N, "insufficient bytes");
        Self::try_from(&bytes[..N]).unwrap()
    }
}

impl FromMidenRepr for [Felt; 4] {
    #[inline(always)]
    fn size_in_felts() -> usize {
        4
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        panic!("field elements have no canonical byte representation")
    }

    #[inline(always)]
    fn from_felts(felts: &[RawFelt]) -> Self {
        [Felt(felts[0]), Felt(felts[1]), Felt(felts[2]), Felt(felts[3])]
    }
}

/// Convert a byte array to an equivalent vector of words
///
/// Given a byte slice laid out like so:
///
/// [b0, b1, b2, b3, b4, b5, b6, b7, .., b31]
///
/// This will produce a vector of words laid out like so:
///
/// [[{b12, ..b15}, {b8..b11}, {b4, ..b7}, {b0, ..b3}], [{b31, ..}, ..]]
///
/// In short, it produces words that when placed on the stack and written to memory word-by-word,
/// the original bytes will be laid out in Miden's memory in the correct order.
pub fn bytes_to_words(bytes: &[u8]) -> Vec<[RawFelt; 4]> {
    // 1. Chunk bytes up into felts
    let mut iter = bytes.iter().array_chunks::<4>();
    let padded_bytes = bytes.len().next_multiple_of(16);
    let num_felts = padded_bytes / 4;
    let mut buf = Vec::with_capacity(num_felts);
    for chunk in iter.by_ref() {
        let n = u32::from_ne_bytes([*chunk[0], *chunk[1], *chunk[2], *chunk[3]]);
        buf.push(n);
    }
    // Zero-pad the buffer to nearest whole element
    if let Some(rest) = iter.into_remainder().filter(|r| r.len() > 0) {
        if rest.len() > 0 {
            let mut n_buf = [0u8; 4];
            for (i, byte) in rest.into_iter().enumerate() {
                n_buf[i] = *byte;
            }
            buf.push(u32::from_ne_bytes(n_buf));
        }
    }
    // Zero-pad the buffer to nearest whole word
    buf.resize(num_felts, 0);
    // Chunk into words, and push them in largest-address first order
    let num_words = num_felts / 4;
    let mut words = Vec::with_capacity(num_words);
    let mut iter = buf.into_iter().map(|elem| RawFelt::new(elem as u64)).array_chunks::<4>();
    for mut word in iter.by_ref() {
        word.reverse();
        words.push(word);
    }
    if let Some(extra) = iter.into_remainder().filter(|r| r.len() > 0) {
        if extra.len() > 0 {
            let mut word = [RawFelt::ZERO; 4];
            for (i, felt) in extra.enumerate() {
                word[i] = felt;
            }
            word.reverse();
            words.push(word);
        }
    }
    words
}

/// Wrapper around `miden_processor::Felt` that implements useful traits that are not implemented
/// for that type.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Felt(pub RawFelt);
impl Felt {
    #[inline]
    pub fn new(value: u64) -> Self {
        Self(RawFelt::new(value))
    }
}

impl<'de> Deserialize<'de> for Felt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        u64::deserialize(deserializer).and_then(|n| {
            if n > RawFelt::MODULUS {
                Err(serde::de::Error::custom(
                    "invalid field element value: exceeds the field modulus",
                ))
            } else {
                RawFelt::try_from(n).map(Felt).map_err(|err| {
                    serde::de::Error::custom(format!("invalid field element value: {err}"))
                })
            }
        })
    }
}

impl clap::builder::ValueParserFactory for Felt {
    type Parser = FeltParser;

    fn value_parser() -> Self::Parser {
        FeltParser
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct FeltParser;
impl clap::builder::TypedValueParser for FeltParser {
    type Value = Felt;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::error::Error> {
        use clap::error::{Error, ErrorKind};

        let value = value.to_str().ok_or_else(|| Error::new(ErrorKind::InvalidUtf8))?.trim();
        value.parse().map_err(|err| Error::raw(ErrorKind::ValueValidation, err))
    }
}

impl core::str::FromStr for Felt {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = if let Some(value) = s.strip_prefix("0x") {
            u64::from_str_radix(value, 16)
                .map_err(|err| format!("invalid field element value: {err}"))?
        } else {
            s.parse::<u64>().map_err(|err| format!("invalid field element value: {err}"))?
        };

        if value > RawFelt::MODULUS {
            Err("invalid field element value: exceeds the field modulus".to_string())
        } else {
            RawFelt::try_from(value).map(Felt)
        }
    }
}

impl From<Felt> for miden_processor::Felt {
    fn from(f: Felt) -> Self {
        f.0
    }
}

impl From<bool> for Felt {
    fn from(b: bool) -> Self {
        Self(RawFelt::from(b as u32))
    }
}

impl From<u8> for Felt {
    fn from(t: u8) -> Self {
        Self(t.into())
    }
}

impl From<i8> for Felt {
    fn from(t: i8) -> Self {
        Self((t as u8).into())
    }
}

impl From<i16> for Felt {
    fn from(t: i16) -> Self {
        Self((t as u16).into())
    }
}

impl From<u16> for Felt {
    fn from(t: u16) -> Self {
        Self(t.into())
    }
}

impl From<i32> for Felt {
    fn from(t: i32) -> Self {
        Self((t as u32).into())
    }
}

impl From<u32> for Felt {
    fn from(t: u32) -> Self {
        Self(t.into())
    }
}

impl From<u64> for Felt {
    fn from(t: u64) -> Self {
        Self(RawFelt::new(t))
    }
}

impl From<i64> for Felt {
    fn from(t: i64) -> Self {
        Self(RawFelt::new(t as u64))
    }
}

// Reverse Felt to Rust types conversion

impl From<Felt> for bool {
    fn from(f: Felt) -> Self {
        f.0.as_int() != 0
    }
}

impl From<Felt> for u8 {
    fn from(f: Felt) -> Self {
        f.0.as_int() as u8
    }
}

impl From<Felt> for i8 {
    fn from(f: Felt) -> Self {
        f.0.as_int() as i8
    }
}

impl From<Felt> for u16 {
    fn from(f: Felt) -> Self {
        f.0.as_int() as u16
    }
}

impl From<Felt> for i16 {
    fn from(f: Felt) -> Self {
        f.0.as_int() as i16
    }
}

impl From<Felt> for u32 {
    fn from(f: Felt) -> Self {
        f.0.as_int() as u32
    }
}

impl From<Felt> for i32 {
    fn from(f: Felt) -> Self {
        f.0.as_int() as i32
    }
}

impl From<Felt> for u64 {
    fn from(f: Felt) -> Self {
        f.0.as_int()
    }
}

impl From<Felt> for i64 {
    fn from(f: Felt) -> Self {
        f.0.as_int() as i64
    }
}

impl Arbitrary for Felt {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        use miden_core::StarkField;
        (0u64..RawFelt::MODULUS).prop_map(|v| Felt(RawFelt::new(v))).boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use miden_core::Word;

    use super::{bytes_to_words, FromMidenRepr, ToMidenRepr};

    #[test]
    fn bool_roundtrip() {
        let encoded = true.to_bytes();
        let decoded = <bool as FromMidenRepr>::from_bytes(&encoded);
        assert!(decoded);

        let encoded = true.to_felts();
        let decoded = <bool as FromMidenRepr>::from_felts(&encoded);
        assert!(decoded);

        let encoded = true.to_words();
        let decoded = <bool as FromMidenRepr>::from_words(&encoded);
        assert!(decoded);

        let mut stack = Vec::default();
        true.push_to_operand_stack(&mut stack);
        let popped = <bool as FromMidenRepr>::pop_from_stack(&mut stack);
        assert!(popped);
    }

    #[test]
    fn u8_roundtrip() {
        let encoded = u8::MAX.to_bytes();
        let decoded = <u8 as FromMidenRepr>::from_bytes(&encoded);
        assert_eq!(decoded, u8::MAX);

        let encoded = u8::MAX.to_felts();
        let decoded = <u8 as FromMidenRepr>::from_felts(&encoded);
        assert_eq!(decoded, u8::MAX);

        let encoded = u8::MAX.to_words();
        let decoded = <u8 as FromMidenRepr>::from_words(&encoded);
        assert_eq!(decoded, u8::MAX);

        let mut stack = Vec::default();
        u8::MAX.push_to_operand_stack(&mut stack);
        let popped = <u8 as FromMidenRepr>::pop_from_stack(&mut stack);
        assert_eq!(popped, u8::MAX);
    }

    #[test]
    fn u16_roundtrip() {
        let encoded = u16::MAX.to_bytes();
        let decoded = <u16 as FromMidenRepr>::from_bytes(&encoded);
        assert_eq!(decoded, u16::MAX);

        let encoded = u16::MAX.to_felts();
        let decoded = <u16 as FromMidenRepr>::from_felts(&encoded);
        assert_eq!(decoded, u16::MAX);

        let encoded = u16::MAX.to_words();
        let decoded = <u16 as FromMidenRepr>::from_words(&encoded);
        assert_eq!(decoded, u16::MAX);

        let mut stack = Vec::default();
        u16::MAX.push_to_operand_stack(&mut stack);
        let popped = <u16 as FromMidenRepr>::pop_from_stack(&mut stack);
        assert_eq!(popped, u16::MAX);
    }

    #[test]
    fn u32_roundtrip() {
        let encoded = u32::MAX.to_bytes();
        let decoded = <u32 as FromMidenRepr>::from_bytes(&encoded);
        assert_eq!(decoded, u32::MAX);

        let encoded = u32::MAX.to_felts();
        let decoded = <u32 as FromMidenRepr>::from_felts(&encoded);
        assert_eq!(decoded, u32::MAX);

        let encoded = u32::MAX.to_words();
        let decoded = <u32 as FromMidenRepr>::from_words(&encoded);
        assert_eq!(decoded, u32::MAX);

        let mut stack = Vec::default();
        u32::MAX.push_to_operand_stack(&mut stack);
        let popped = <u32 as FromMidenRepr>::pop_from_stack(&mut stack);
        assert_eq!(popped, u32::MAX);
    }

    #[test]
    fn u64_roundtrip() {
        let encoded = u64::MAX.to_bytes();
        let decoded = <u64 as FromMidenRepr>::from_bytes(&encoded);
        assert_eq!(decoded, u64::MAX);

        let encoded = u64::MAX.to_felts();
        let decoded = <u64 as FromMidenRepr>::from_felts(&encoded);
        assert_eq!(decoded, u64::MAX);

        let encoded = u64::MAX.to_words();
        let decoded = <u64 as FromMidenRepr>::from_words(&encoded);
        assert_eq!(decoded, u64::MAX);

        let mut stack = Vec::default();
        u64::MAX.push_to_operand_stack(&mut stack);
        let popped = <u64 as FromMidenRepr>::pop_from_stack(&mut stack);
        assert_eq!(popped, u64::MAX);
    }

    #[test]
    fn u128_roundtrip() {
        let encoded = u128::MAX.to_bytes();
        let decoded = <u128 as FromMidenRepr>::from_bytes(&encoded);
        assert_eq!(decoded, u128::MAX);

        let encoded = u128::MAX.to_felts();
        let decoded = <u128 as FromMidenRepr>::from_felts(&encoded);
        assert_eq!(decoded, u128::MAX);

        let encoded = u128::MAX.to_words();
        let decoded = <u128 as FromMidenRepr>::from_words(&encoded);
        assert_eq!(decoded, u128::MAX);

        let mut stack = Vec::default();
        u128::MAX.push_to_operand_stack(&mut stack);
        let popped = <u128 as FromMidenRepr>::pop_from_stack(&mut stack);
        assert_eq!(popped, u128::MAX);
    }

    #[test]
    fn byte_array_roundtrip() {
        let bytes = [0, 1, 2, 3, 4, 5, 6, 7];

        let encoded = bytes.to_felts();
        let decoded = <[u8; 8] as FromMidenRepr>::from_felts(&encoded);
        assert_eq!(decoded, bytes);

        let encoded = bytes.to_words();
        let decoded = <[u8; 8] as FromMidenRepr>::from_words(&encoded);
        assert_eq!(decoded, bytes);

        let mut stack = Vec::default();
        bytes.push_to_operand_stack(&mut stack);
        let popped = <[u8; 8] as FromMidenRepr>::pop_from_stack(&mut stack);
        assert_eq!(popped, bytes);
    }

    #[test]
    fn bytes_to_words_test() {
        let bytes = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let words = bytes_to_words(&bytes);
        assert_eq!(words.len(), 2);
        // Words should be in little-endian order, elements of the word should be in big-endian
        assert_eq!(words[0][3].as_int() as u32, u32::from_ne_bytes([1, 2, 3, 4]));
        assert_eq!(words[0][2].as_int() as u32, u32::from_ne_bytes([5, 6, 7, 8]));
        assert_eq!(words[0][1].as_int() as u32, u32::from_ne_bytes([9, 10, 11, 12]));
        assert_eq!(words[0][0].as_int() as u32, u32::from_ne_bytes([13, 14, 15, 16]));

        // Make sure bytes_to_words and to_words agree
        let to_words_output = bytes.to_words();
        assert_eq!(Word::new(words[0]), to_words_output[0]);
    }

    #[test]
    fn bytes_from_words_test() {
        let bytes = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let words_as_bytes = bytes_to_words(&bytes);

        let words = vec![Word::new(words_as_bytes[0]), Word::new(words_as_bytes[1])];

        let out = <[u8; 32] as FromMidenRepr>::from_words(&words);

        assert_eq!(&out, &bytes);
    }
}
