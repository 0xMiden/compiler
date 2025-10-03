use std::borrow::Cow;

use miden_core::{Felt, Word};
use miden_debug::ToMidenRepr;

/// An [Initializer] represents a known initialization pattern that is handled by the compiler-
/// emitted test harness, when enabled.
///
/// These can be used to initialize global program state when the program starts, to set things up
/// for a specific test case.
pub enum Initializer<'a> {
    /// Write `value` to memory starting at `addr`
    Value {
        /// The address (in byte-addressable space) to write `value` to
        addr: u32,
        /// The value to be written
        value: Box<dyn ToMidenRepr>,
    },
    /// Write `bytes` to memory starting at `addr`
    MemoryBytes {
        /// The address (in byte-addressable space) to write `bytes` to
        addr: u32,
        /// The bytes to be written
        bytes: &'a [u8],
    },
    /// Write `felts` to memory starting at `addr`
    MemoryFelts {
        /// The address (in element-addressable space) to write `felts` to
        addr: u32,
        /// The field elements to be written
        felts: Cow<'a, [Felt]>,
    },
    /// Write `words` to memory starting at `addr`
    #[allow(dead_code)]
    MemoryWords {
        /// The address (in element-addressable space) to write `words` to
        addr: u32,
        /// The words to be written
        words: Cow<'a, [Word]>,
    },
}

impl Initializer<'_> {
    /// Get the address this initializes, in element-addressable space
    pub fn element_addr(&self) -> u32 {
        match self {
            Self::Value { addr, .. } | Self::MemoryBytes { addr, .. } => *addr / 4,
            Self::MemoryFelts { addr, .. } | Self::MemoryWords { addr, .. } => *addr,
        }
    }
}
