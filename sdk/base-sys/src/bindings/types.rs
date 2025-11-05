use miden_stdlib_sys::{Felt, Word};

#[allow(unused)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AccountId {
    pub prefix: Felt,
    pub suffix: Felt,
}

impl AccountId {
    /// Creates a new AccountId from prefix and suffix Felt values
    pub fn from(prefix: Felt, suffix: Felt) -> Self {
        Self { prefix, suffix }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Asset {
    pub inner: Word,
}

impl Asset {
    pub fn new(word: impl Into<Word>) -> Self {
        Asset { inner: word.into() }
    }

    pub fn as_word(&self) -> &Word {
        &self.inner
    }

    #[inline]
    pub(crate) fn reverse(&self) -> Self {
        Self {
            inner: self.inner.reverse(),
        }
    }
}

impl From<Word> for Asset {
    fn from(value: Word) -> Self {
        Self::new(value)
    }
}

impl From<[Felt; 4]> for Asset {
    fn from(value: [Felt; 4]) -> Self {
        Asset::new(Word::from(value))
    }
}

impl From<Asset> for Word {
    fn from(val: Asset) -> Self {
        val.inner
    }
}

impl AsRef<Word> for Asset {
    fn as_ref(&self) -> &Word {
        &self.inner
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Recipient {
    pub inner: Word,
}

impl From<[Felt; 4]> for Recipient {
    fn from(value: [Felt; 4]) -> Self {
        Recipient {
            inner: Word::from(value),
        }
    }
}

impl From<Word> for Recipient {
    fn from(value: Word) -> Self {
        Recipient { inner: value }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Tag {
    pub inner: Felt,
}

impl From<Felt> for Tag {
    fn from(value: Felt) -> Self {
        Tag { inner: value }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct NoteIdx {
    pub inner: Felt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct NoteType {
    pub inner: Felt,
}

impl From<Felt> for NoteType {
    fn from(value: Felt) -> Self {
        NoteType { inner: value }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct StorageCommitmentRoot(Word);

impl StorageCommitmentRoot {
    #[inline]
    pub(crate) fn reverse(&self) -> StorageCommitmentRoot {
        Self(self.0.reverse())
    }
}
