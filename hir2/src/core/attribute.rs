use alloc::collections::BTreeMap;
use core::{borrow::Borrow, fmt};

use midenc_hir_symbol::Symbol;

pub mod attributes {
    use midenc_hir_symbol::symbols;

    use super::*;

    /// This attribute indicates that the decorated function is the entrypoint
    /// for its containing program, regardless of what module it is defined in.
    pub const ENTRYPOINT: Attribute = Attribute {
        name: symbols::Entrypoint,
        value: AttributeValue::Unit,
    };
}

/// An [AttributeSet] is a uniqued collection of attributes associated with some IR entity
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct AttributeSet(BTreeMap<Symbol, AttributeValue>);
impl FromIterator<Attribute> for AttributeSet {
    fn from_iter<T>(attrs: T) -> Self
    where
        T: IntoIterator<Item = Attribute>,
    {
        let mut map = BTreeMap::default();
        for attr in attrs.into_iter() {
            map.insert(attr.name, attr.value);
        }
        Self(map)
    }
}
impl FromIterator<(Symbol, AttributeValue)> for AttributeSet {
    fn from_iter<T>(attrs: T) -> Self
    where
        T: IntoIterator<Item = (Symbol, AttributeValue)>,
    {
        let mut map = BTreeMap::default();
        for (name, value) in attrs.into_iter() {
            map.insert(name, value);
        }
        Self(map)
    }
}
impl AttributeSet {
    /// Get a new, empty [AttributeSet]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new [Attribute] in this set by `name` and `value`
    pub fn insert(&mut self, name: impl Into<Symbol>, value: impl Into<AttributeValue>) {
        self.0.insert(name.into(), value.into());
    }

    /// Adds `attr` to this set
    pub fn set(&mut self, attr: Attribute) {
        self.0.insert(attr.name, attr.value);
    }

    /// Remove an [Attribute] by name from this set
    pub fn remove<Q>(&mut self, name: &Q)
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.0.remove(name);
    }

    /// Determine if the named [Attribute] is present in this set
    pub fn has<Q>(&self, key: &Q) -> bool
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.0.contains_key(key)
    }

    /// Get the [AttributeValue] associated with the named [Attribute]
    pub fn get<Q>(&self, key: &Q) -> Option<&AttributeValue>
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.0.get(key)
    }

    /// Get the value associated with the named [Attribute] as a boolean, or `None`.
    pub fn get_bool<Q>(&self, key: &Q) -> Option<bool>
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.0.get(key).and_then(|v| v.as_bool())
    }

    /// Get the value associated with the named [Attribute] as an integer, or `None`.
    pub fn get_int<Q>(&self, key: &Q) -> Option<isize>
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.0.get(key).and_then(|v| v.as_int())
    }

    /// Get the value associated with the named [Attribute] as a [Symbol], or `None`.
    pub fn get_symbol<Q>(&self, key: &Q) -> Option<Symbol>
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.0.get(key).and_then(|v| v.as_symbol())
    }

    /// Iterate over each [Attribute] in this set
    pub fn iter(&self) -> impl Iterator<Item = Attribute> + '_ {
        self.0.iter().map(|(k, v)| Attribute {
            name: *k,
            value: *v,
        })
    }
}

/// An [Attribute] associates some data with a well-known identifier (name).
///
/// Attributes are used for representing metadata that helps guide compilation,
/// but which is not part of the code itself. For example, `cfg` flags in Rust
/// are an example of something which you could represent using an [Attribute].
/// They can also be used to store documentation, source locations, and more.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Attribute {
    /// The name of this attribute
    pub name: Symbol,
    /// The value associated with this attribute
    pub value: AttributeValue,
}
impl Attribute {
    pub fn new(name: impl Into<Symbol>, value: impl Into<AttributeValue>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}
impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.value {
            AttributeValue::Unit => write!(f, "#[{}]", self.name.as_str()),
            value => write!(f, "#[{}({value})]", &self.name),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttributeValue {
    /// No concrete value (i.e. presence of the attribute is significant)
    Unit,
    /// A boolean value
    Bool(bool),
    /// A signed integer
    Int(isize),
    /// An interned string
    String(Symbol),
}
impl AttributeValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<isize> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_symbol(&self) -> Option<Symbol> {
        match self {
            Self::String(value) => Some(*value),
            _ => None,
        }
    }
}
impl fmt::Display for AttributeValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unit => f.write_str("()"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::String(value) => write!(f, "\"{}\"", value.as_str().escape_default()),
        }
    }
}
impl From<()> for AttributeValue {
    fn from(_: ()) -> Self {
        Self::Unit
    }
}
impl From<bool> for AttributeValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<isize> for AttributeValue {
    fn from(value: isize) -> Self {
        Self::Int(value)
    }
}
impl From<&str> for AttributeValue {
    fn from(value: &str) -> Self {
        Self::String(Symbol::intern(value))
    }
}
impl From<String> for AttributeValue {
    fn from(value: String) -> Self {
        Self::String(Symbol::intern(value.as_str()))
    }
}
impl From<u8> for AttributeValue {
    fn from(value: u8) -> Self {
        Self::Int(value as isize)
    }
}
impl From<i8> for AttributeValue {
    fn from(value: i8) -> Self {
        Self::Int(value as isize)
    }
}
impl From<u16> for AttributeValue {
    fn from(value: u16) -> Self {
        Self::Int(value as isize)
    }
}
impl From<i16> for AttributeValue {
    fn from(value: i16) -> Self {
        Self::Int(value as isize)
    }
}
impl From<u32> for AttributeValue {
    fn from(value: u32) -> Self {
        Self::Int(value as isize)
    }
}
impl From<i32> for AttributeValue {
    fn from(value: i32) -> Self {
        Self::Int(value as isize)
    }
}
impl TryFrom<usize> for AttributeValue {
    type Error = core::num::TryFromIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Ok(Self::Int(value.try_into()?))
    }
}
impl TryFrom<u64> for AttributeValue {
    type Error = core::num::TryFromIntError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Ok(Self::Int(value.try_into()?))
    }
}
impl TryFrom<i64> for AttributeValue {
    type Error = core::num::TryFromIntError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(Self::Int(value.try_into()?))
    }
}
