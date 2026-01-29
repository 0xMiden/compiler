use core::ops::{Index, IndexMut};

use super::felt::Felt;
use crate::felt;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(C, align(16))]
pub struct Word {
    pub inner: (Felt, Felt, Felt, Felt),
}
impl Word {
    pub const fn new(word: [Felt; 4]) -> Self {
        Self {
            inner: (word[0], word[1], word[2], word[3]),
        }
    }

    /// Converts each of the `values` to `Felt`.
    ///
    /// # Panics
    ///
    /// Panics if a value is larger than `Felt::M`.
    #[inline(always)]
    pub fn from_u64_unchecked(a: u64, b: u64, c: u64, d: u64) -> Self {
        Self {
            inner: (
                Felt::from_u64_unchecked(a),
                Felt::from_u64_unchecked(b),
                Felt::from_u64_unchecked(c),
                Felt::from_u64_unchecked(d),
            ),
        }
    }

    #[inline(always)]
    pub fn reverse(&self) -> Word {
        Word {
            inner: (self.inner.3, self.inner.2, self.inner.1, self.inner.0),
        }
    }
}
impl From<[Felt; 4]> for Word {
    fn from(word: [Felt; 4]) -> Self {
        Self {
            inner: (word[0], word[1], word[2], word[3]),
        }
    }
}
impl From<Word> for [Felt; 4] {
    #[inline(always)]
    fn from(word: Word) -> Self {
        [word.inner.0, word.inner.1, word.inner.2, word.inner.3]
    }
}
impl From<&Word> for [Felt; 4] {
    #[inline(always)]
    fn from(word: &Word) -> Self {
        [word.inner.0, word.inner.1, word.inner.2, word.inner.3]
    }
}
impl From<Felt> for Word {
    fn from(value: Felt) -> Self {
        Word {
            inner: (felt!(0), felt!(0), felt!(0), value),
        }
    }
}
impl From<Word> for Felt {
    fn from(value: Word) -> Self {
        value.inner.3
    }
}
impl Index<usize> for Word {
    type Output = Felt;

    #[inline(always)]
    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.inner.0,
            1 => &self.inner.1,
            2 => &self.inner.2,
            3 => &self.inner.3,
            _ => unreachable!(),
        }
    }
}
impl IndexMut<usize> for Word {
    #[inline(always)]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            0 => &mut self.inner.0,
            1 => &mut self.inner.1,
            2 => &mut self.inner.2,
            3 => &mut self.inner.3,
            _ => unreachable!(),
        }
    }
}

impl AsRef<Word> for Word {
    fn as_ref(&self) -> &Word {
        self
    }
}
