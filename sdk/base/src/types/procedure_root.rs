use miden_stdlib_sys::Word;

/// The MAST root of a procedure of an account interface.
///
/// This is a wrapper around `Word` which contains 4 field elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ProcedureRoot {
    pub inner: Word,
}

impl From<Word> for ProcedureRoot {
    /// Makes a procedure root from a raw word.
    ///
    /// The word is not validated: the caller must supply the MAST root of an existing procedure.
    fn from(word: Word) -> Self {
        Self { inner: word }
    }
}

impl From<ProcedureRoot> for Word {
    fn from(root: ProcedureRoot) -> Self {
        root.inner
    }
}
