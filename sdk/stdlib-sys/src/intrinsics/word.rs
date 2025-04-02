use core::ops::{Index, IndexMut};

use super::felt::Felt;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(C, align(32))]
pub struct Word {
    pub inner: (Felt, Felt, Felt, Felt),
}
impl Word {
    pub const fn new(word: [Felt; 4]) -> Self {
        Self {
            inner: (word[0], word[1], word[2], word[3]),
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
