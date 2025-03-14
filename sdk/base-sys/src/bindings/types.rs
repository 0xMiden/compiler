use miden_stdlib_sys::{Felt, Word};

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct AccountId(Felt);

impl AccountId {
    #[inline(always)]
    pub const fn as_felt(&self) -> Felt {
        self.0
    }
}

impl From<AccountId> for Felt {
    fn from(account_id: AccountId) -> Felt {
        account_id.0
    }
}

#[repr(transparent)]
pub struct CoreAsset {
    pub inner: Word,
}

impl CoreAsset {
    pub fn new(word: impl Into<Word>) -> Self {
        CoreAsset { inner: word.into() }
    }

    pub fn as_word(&self) -> &Word {
        &self.inner
    }
}

#[repr(transparent)]
pub struct Recipient {
    pub inner: Word,
}

#[repr(transparent)]
pub struct Tag {
    pub inner: Felt,
}

#[repr(transparent)]
pub struct NoteId(pub(crate) Felt);

#[repr(transparent)]
pub struct NoteType {
    pub inner: Felt,
}

#[repr(transparent)]
pub struct StorageCommitmentRoot(Word);
