#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod sync;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use core::{fmt, mem, ops::Deref, str};

pub mod symbols {
    include!(env!("SYMBOLS_RS"));
}

static SYMBOL_TABLE: sync::LazyLock<SymbolTable> = sync::LazyLock::new(SymbolTable::default);

#[derive(Default)]
struct SymbolTable {
    interner: sync::RwLock<Interner>,
}

/// A symbol is an interned string.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(SymbolIndex);

#[cfg(feature = "serde")]
impl serde::Serialize for Symbol {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Symbol {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SymbolVisitor;
        impl<'de> serde::de::Visitor<'de> for SymbolVisitor {
            type Value = Symbol;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("symbol")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Symbol::intern(v))
            }
        }
        deserializer.deserialize_str(SymbolVisitor)
    }
}

impl Symbol {
    #[inline]
    pub const fn new(n: u32) -> Self {
        Self(SymbolIndex::new(n))
    }

    /// Maps a string to its interned representation.
    pub fn intern(string: impl ToString) -> Self {
        let string = string.to_string();
        with_interner(|interner| interner.intern(string))
    }

    pub fn as_str(self) -> &'static str {
        with_read_only_interner(|interner| unsafe {
            // This is safe because the interned string will live for the
            // lifetime of the program
            mem::transmute::<&str, &'static str>(interner.get(self))
        })
    }

    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0.as_u32()
    }

    #[inline]
    pub fn as_usize(self) -> usize {
        self.0.as_usize()
    }

    /// Returns true if this symbol is a keyword in the IR textual format
    #[inline]
    pub fn is_keyword(self) -> bool {
        symbols::is_keyword(self)
    }
}
impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}({:?})", self, self.0)
    }
}
impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.as_str(), f)
    }
}
impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}
impl AsRef<str> for Symbol {
    #[inline(always)]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl core::borrow::Borrow<str> for Symbol {
    #[inline(always)]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
impl<T: Deref<Target = str>> PartialEq<T> for Symbol {
    fn eq(&self, other: &T) -> bool {
        self.as_str() == other.deref()
    }
}
impl From<&'static str> for Symbol {
    fn from(s: &'static str) -> Self {
        with_interner(|interner| interner.insert(s))
    }
}
impl From<String> for Symbol {
    fn from(s: String) -> Self {
        Self::intern(s)
    }
}
impl From<Box<str>> for Symbol {
    fn from(s: Box<str>) -> Self {
        Self::intern(s)
    }
}
impl From<alloc::borrow::Cow<'static, str>> for Symbol {
    fn from(s: alloc::borrow::Cow<'static, str>) -> Self {
        use alloc::borrow::Cow;
        match s {
            Cow::Borrowed(s) => s.into(),
            Cow::Owned(s) => Self::intern(s),
        }
    }
}
#[cfg(feature = "compact_str")]
impl From<compact_str::CompactString> for Symbol {
    fn from(s: compact_str::CompactString) -> Self {
        Self::intern(s.into_string())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SymbolIndex(u32);
impl SymbolIndex {
    // shave off 256 indices at the end to allow space for packing these indices into enums
    pub const MAX_AS_U32: u32 = 0xffff_ff00;

    #[inline]
    const fn new(n: u32) -> Self {
        assert!(n <= Self::MAX_AS_U32, "out of range value used");

        SymbolIndex(n)
    }

    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    #[inline]
    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}
impl From<SymbolIndex> for u32 {
    #[inline]
    fn from(v: SymbolIndex) -> u32 {
        v.as_u32()
    }
}
impl From<SymbolIndex> for usize {
    #[inline]
    fn from(v: SymbolIndex) -> usize {
        v.as_usize()
    }
}

struct Interner {
    pub names: BTreeMap<&'static str, Symbol>,
    pub strings: Vec<&'static str>,
}

impl Default for Interner {
    fn default() -> Self {
        let mut this = Self {
            names: BTreeMap::default(),
            strings: Vec::with_capacity(symbols::__SYMBOLS.len()),
        };
        for (sym, s) in symbols::__SYMBOLS {
            this.names.insert(s, *sym);
            this.strings.push(s);
        }
        this
    }
}

impl Interner {
    pub fn intern(&mut self, string: String) -> Symbol {
        if let Some(&name) = self.names.get(string.as_str()) {
            return name;
        }

        let name = Symbol::new(self.strings.len() as u32);

        let string = string.into_boxed_str();
        let string: &'static str = Box::leak(string);
        self.strings.push(string);
        self.names.insert(string, name);
        name
    }

    pub fn insert(&mut self, s: &'static str) -> Symbol {
        if let Some(&name) = self.names.get(s) {
            return name;
        }
        let name = Symbol::new(self.strings.len() as u32);
        self.strings.push(s);
        self.names.insert(s, name);
        name
    }

    pub fn get(&self, symbol: Symbol) -> &'static str {
        self.strings[symbol.0.as_usize()]
    }
}

// If an interner exists, return it. Otherwise, prepare a fresh one.
#[inline]
fn with_interner<T, F: FnOnce(&mut Interner) -> T>(f: F) -> T {
    let mut table = SYMBOL_TABLE.interner.write();
    f(&mut table)
}

#[inline]
fn with_read_only_interner<T, F: FnOnce(&Interner) -> T>(f: F) -> T {
    let table = SYMBOL_TABLE.interner.read();
    f(&table)
}
