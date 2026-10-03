//! Felt and word constants read from a package manifest.
//!
//! A manifest records a felt constant as its canonical `u64`, and there is no `const fn` that
//! turns one into a [`Felt`] on the Miden target, where the Rust and VM representations of a felt
//! differ. So a generated constant holds the canonical value as an intermediate form, and
//! [`FeltConstant::get`] / [`WordConstant::get`] perform the conversion at the use site.

use crate::{Felt, Word};

/// A felt constant, held as its canonical `u64` until [`Self::get`] converts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FeltConstant(u64);

impl FeltConstant {
    /// Wraps a canonical felt value: a `u64` below the field modulus, as the assembler wrote it
    /// into the manifest.
    pub const fn new(canonical: u64) -> Self {
        Self(canonical)
    }

    /// The canonical `u64` value.
    pub const fn canonical(self) -> u64 {
        self.0
    }

    /// The constant as a [`Felt`].
    pub fn get(self) -> Felt {
        Felt::new_unchecked(self.0)
    }
}

/// A word constant, held as four canonical `u64`s until [`Self::get`] converts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WordConstant([u64; 4]);

impl WordConstant {
    /// Wraps four canonical felt values, element by element.
    pub const fn new(canonical: [u64; 4]) -> Self {
        Self(canonical)
    }

    /// The canonical values, element by element.
    pub const fn canonical(self) -> [u64; 4] {
        self.0
    }

    /// The constant as a [`Word`].
    pub fn get(self) -> Word {
        Word::new(self.0.map(Felt::new_unchecked))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_felt_constant_converts_to_the_felt_it_was_written_from() {
        const LIMIT: FeltConstant = FeltConstant::new(9223372034707292160);
        assert_eq!(LIMIT.canonical(), 9223372034707292160);
        assert_eq!(LIMIT.get(), Felt::new(9223372034707292160).unwrap());
    }

    #[test]
    fn a_word_constant_converts_element_by_element() {
        const SLOT: WordConstant = WordConstant::new([1, 2, 3, 4]);
        assert_eq!(SLOT.canonical(), [1, 2, 3, 4]);
        let word = SLOT.get();
        assert_eq!(word, Word::new([1u64, 2, 3, 4].map(|v| Felt::new(v).unwrap())));
    }
}
