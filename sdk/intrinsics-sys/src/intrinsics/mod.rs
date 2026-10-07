//! The compiler intrinsics: operations the Wasm frontend recognizes by their link name and lowers
//! to Miden VM instructions directly, rather than to a call into a Miden package.

pub use miden_field::Word;

pub use self::felt::{Felt, assert, assert_eq, assertz};
pub use crate::WordAligned;

pub mod advice;
pub mod debug;
pub mod felt;
