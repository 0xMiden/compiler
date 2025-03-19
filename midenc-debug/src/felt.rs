use std::collections::VecDeque;

use miden_core::StarkField;
use miden_processor::Felt as RawFelt;
use proptest::{
    arbitrary::Arbitrary,
    strategy::{BoxedStrategy, Strategy},
};
use serde::Deserialize;

pub trait PushToStack: Sized {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        let mut ptr = self as *const Self as *const u8;
        let mut num_bytes = core::mem::size_of::<Self>();
        let mut buf = Vec::with_capacity(num_bytes / core::mem::size_of::<u32>());
        while num_bytes > 0 {
            let mut next = [0u8; 4];
            let consume = core::cmp::min(4, num_bytes);
            unsafe {
                ptr.copy_to_nonoverlapping(next.as_mut_ptr(), consume);
                ptr = ptr.byte_add(consume);
            }
            num_bytes -= consume;
            buf.push(RawFelt::new(u32::from_be_bytes(next) as u64));
        }

        for item in buf.into_iter().rev() {
            stack.push(item);
        }
    }
}

pub trait PopFromStack: Sized {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        use core::mem::MaybeUninit;

        let mut num_bytes = core::mem::size_of::<Self>();
        let mut result = MaybeUninit::<Self>::uninit();
        let mut ptr = result.as_mut_ptr() as *mut u8;
        while num_bytes > 0 {
            let next = stack.pop_front().expect("expected more operand stack elements");
            let next_bytes = (next.0.as_int() as u32).to_be_bytes();
            let consume = core::cmp::min(4, num_bytes);
            unsafe {
                next_bytes.as_ptr().copy_to_nonoverlapping(ptr, consume);
                ptr = ptr.byte_add(consume);
            }
            num_bytes -= consume;
        }
        Some(unsafe { result.assume_init() })
    }
}

impl PushToStack for bool {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u64))
    }
}
impl PopFromStack for bool {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        Some(stack.pop_front().unwrap().0.as_int() != 0)
    }
}

impl PushToStack for u8 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u64))
    }
}
impl PopFromStack for u8 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        Some(stack.pop_front().unwrap().0.as_int() as u8)
    }
}

impl PushToStack for i8 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u8 as u64))
    }
}
impl PopFromStack for i8 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        Some(stack.pop_front().unwrap().0.as_int() as i8)
    }
}

impl PushToStack for u16 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u64))
    }
}
impl PopFromStack for u16 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        Some(stack.pop_front().unwrap().0.as_int() as u16)
    }
}

impl PushToStack for i16 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u16 as u64))
    }
}
impl PopFromStack for i16 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        Some(stack.pop_front().unwrap().0.as_int() as i16)
    }
}

impl PushToStack for u32 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u64))
    }
}
impl PopFromStack for u32 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        Some(stack.pop_front().unwrap().0.as_int() as u32)
    }
}

impl PushToStack for i32 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(RawFelt::new(*self as u32 as u64))
    }
}
impl PopFromStack for i32 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        Some(stack.pop_front().unwrap().0.as_int() as i32)
    }
}

impl PushToStack for u64 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        let lo = self.rem_euclid(2u64.pow(32));
        let hi = self.div_euclid(2u64.pow(32));
        stack.push(RawFelt::new(lo));
        stack.push(RawFelt::new(hi));
    }
}
impl PopFromStack for u64 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        let hi = stack.pop_front().unwrap().0.as_int() * 2u64.pow(32);
        let lo = stack.pop_front().unwrap().0.as_int();
        Some(hi + lo)
    }
}

impl PushToStack for i64 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        (*self as u64).try_push(stack)
    }
}
impl PopFromStack for i64 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        u64::try_pop(stack).map(|value| value as i64)
    }
}

impl PushToStack for u128 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        let lo = self.rem_euclid(2u128.pow(64));
        let hi = self.div_euclid(2u128.pow(64));
        (lo as u64).try_push(stack);
        (hi as u64).try_push(stack);
    }
}
impl PopFromStack for u128 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        let hi = (u64::try_pop(stack).unwrap() as u128) * 2u128.pow(64);
        let lo = u64::try_pop(stack).unwrap() as u128;
        Some(hi + lo)
    }
}

impl PushToStack for i128 {
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        (*self as u128).try_push(stack)
    }
}
impl PopFromStack for i128 {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        u128::try_pop(stack).map(|value| value as i128)
    }
}

impl PushToStack for RawFelt {
    #[inline(always)]
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(*self);
    }
}
impl PopFromStack for RawFelt {
    #[inline(always)]
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        Some(stack.pop_front()?.0)
    }
}

impl PushToStack for [RawFelt; 4] {
    #[inline(always)]
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.extend(self.iter().copied().rev());
    }
}
impl PopFromStack for [RawFelt; 4] {
    #[inline(always)]
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        let a = stack.pop_front()?;
        let b = stack.pop_front()?;
        let c = stack.pop_front()?;
        let d = stack.pop_front()?;
        Some([a.0, b.0, c.0, d.0])
    }
}

impl PushToStack for Felt {
    #[inline(always)]
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.push(self.0);
    }
}
impl PopFromStack for Felt {
    #[inline(always)]
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        stack.pop_front()
    }
}

impl PushToStack for [Felt; 4] {
    #[inline(always)]
    fn try_push(&self, stack: &mut Vec<RawFelt>) {
        stack.extend(self.iter().map(|f| f.0).rev());
    }
}
impl PopFromStack for [Felt; 4] {
    #[inline(always)]
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        let a = stack.pop_front()?;
        let b = stack.pop_front()?;
        let c = stack.pop_front()?;
        let d = stack.pop_front()?;
        Some([a, b, c, d])
    }
}

impl<const N: usize> PopFromStack for [u8; N] {
    fn try_pop(stack: &mut VecDeque<Felt>) -> Option<Self> {
        use midenc_hir::FieldElement;
        let mut out = [0u8; N];

        let chunk_size = (out.len() / 4) + (out.len() % 4 > 0) as usize;
        for i in 0..chunk_size {
            let elem: u32 = PopFromStack::try_pop(stack)?;
            let bytes = elem.to_le_bytes();
            let offset = i * 4;
            if offset + 3 < N {
                out[offset] = bytes[0];
                out[offset + 1] = bytes[1];
                out[offset + 2] = bytes[2];
                out[offset + 3] = bytes[3];
            } else if offset + 2 < N {
                out[offset] = bytes[0];
                out[offset + 1] = bytes[1];
                out[offset + 2] = bytes[2];
                break;
            } else if offset + 1 < N {
                out[offset] = bytes[0];
                out[offset + 1] = bytes[1];
                break;
            } else if offset < N {
                out[offset] = bytes[0];
                break;
            } else {
                break;
            }
        }

        Some(out)
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
/// [[{b0, ..b3}, {b4, ..b7}, {b8..b11}, {b12, ..b15}], ..]
///
/// In other words, it produces words that when placed on the stack and written to memory
/// word-by-word, that memory will be laid out in the correct byte order.
pub fn bytes_to_words(bytes: &[u8]) -> Vec<[RawFelt; 4]> {
    // 1. Chunk bytes up into felts
    let mut iter = bytes.iter().array_chunks::<4>();
    let buf_size = (bytes.len() / 4) + (bytes.len() % 4 > 0) as usize;
    let padding = buf_size % 8;
    let mut buf = Vec::with_capacity(buf_size + padding);
    for chunk in iter.by_ref() {
        let n = u32::from_le_bytes([*chunk[0], *chunk[1], *chunk[2], *chunk[3]]);
        buf.push(n);
    }
    // Zero-pad the buffer to nearest whole element
    if let Some(rest) = iter.into_remainder() {
        let mut n_buf = [0u8; 4];
        for (i, byte) in rest.into_iter().enumerate() {
            n_buf[i] = *byte;
        }
        buf.push(u32::from_le_bytes(n_buf));
    }
    // Zero-pad the buffer to nearest whole word
    let padded_buf_size = buf_size + padding;
    buf.resize(padded_buf_size, 0);
    // Chunk into words, and push them in largest-address first order
    let word_size = (padded_buf_size / 4) + (padded_buf_size % 4 > 0) as usize;
    let mut words = Vec::with_capacity(word_size);
    for mut word_chunk in buf.into_iter().map(|elem| RawFelt::new(elem as u64)).array_chunks::<4>()
    {
        words.push(word_chunk);
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

    use super::{bytes_to_words, PopFromStack};

    #[test]
    fn bytes_to_words_test() {
        let bytes = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let words = bytes_to_words(&bytes);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0][0].as_int() as u32, u32::from_le_bytes([1, 2, 3, 4]));
        assert_eq!(words[0][1].as_int() as u32, u32::from_le_bytes([5, 6, 7, 8]));
        assert_eq!(words[0][2].as_int() as u32, u32::from_le_bytes([9, 10, 11, 12]));
        assert_eq!(words[0][3].as_int() as u32, u32::from_le_bytes([13, 14, 15, 16]));
    }

    #[test]
    fn bytes_from_words_test() {
        let bytes = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let words = bytes_to_words(&bytes);
        let mut stack = VecDeque::from_iter(words.into_iter().flatten().map(super::Felt));
        let out: [u8; 32] = PopFromStack::try_pop(&mut stack).unwrap();
        assert_eq!(&out, &bytes);
    }
}
