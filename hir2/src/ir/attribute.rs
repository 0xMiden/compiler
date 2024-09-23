use alloc::collections::BTreeMap;
use core::{any::Any, borrow::Borrow, fmt};

use super::interner::Symbol;

pub mod attributes {
    use midenc_hir_symbol::symbols;

    use super::*;

    /// This attribute indicates that the decorated function is the entrypoint
    /// for its containing program, regardless of what module it is defined in.
    pub const ENTRYPOINT: Attribute = Attribute {
        name: symbols::Entrypoint,
        value: None,
    };
}

/// An [AttributeSet] is a uniqued collection of attributes associated with some IR entity
#[derive(Debug, Default)]
pub struct AttributeSet(Vec<Attribute>);
impl FromIterator<Attribute> for AttributeSet {
    fn from_iter<T>(attrs: T) -> Self
    where
        T: IntoIterator<Item = Attribute>,
    {
        let mut map = BTreeMap::default();
        for attr in attrs.into_iter() {
            map.insert(attr.name, attr.value);
        }
        Self(map.into_iter().map(|(name, value)| Attribute { name, value }).collect())
    }
}
impl FromIterator<(Symbol, Option<Box<dyn AttributeValue>>)> for AttributeSet {
    fn from_iter<T>(attrs: T) -> Self
    where
        T: IntoIterator<Item = (Symbol, Option<Box<dyn AttributeValue>>)>,
    {
        let mut map = BTreeMap::default();
        for (name, value) in attrs.into_iter() {
            map.insert(name, value);
        }
        Self(map.into_iter().map(|(name, value)| Attribute { name, value }).collect())
    }
}
impl AttributeSet {
    /// Get a new, empty [AttributeSet]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new [Attribute] in this set by `name` and `value`
    pub fn insert(&mut self, name: impl Into<Symbol>, value: Option<impl AttributeValue>) {
        let name = name.into();
        match self.0.binary_search_by_key(&name, |attr| attr.name) {
            Ok(index) => {
                self.0[index].value = value.map(|v| Box::new(v) as Box<dyn AttributeValue>);
            }
            Err(index) => {
                let value = value.map(|v| Box::new(v) as Box<dyn AttributeValue>);
                if index == self.0.len() {
                    self.0.push(Attribute { name, value });
                } else {
                    self.0.insert(index, Attribute { name, value });
                }
            }
        }
    }

    /// Adds `attr` to this set
    pub fn set(&mut self, attr: Attribute) {
        match self.0.binary_search_by_key(&attr.name, |attr| attr.name) {
            Ok(index) => {
                self.0[index].value = attr.value;
            }
            Err(index) => {
                if index == self.0.len() {
                    self.0.push(attr);
                } else {
                    self.0.insert(index, attr);
                }
            }
        }
    }

    /// Remove an [Attribute] by name from this set
    pub fn remove<Q>(&mut self, name: &Q)
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let name = name.borrow();
        match self.0.binary_search_by(|attr| name.cmp(attr.name.borrow()).reverse()) {
            Ok(index) if index + 1 == self.0.len() => {
                self.0.pop();
            }
            Ok(index) => {
                self.0.remove(index);
            }
            Err(_) => (),
        }
    }

    /// Determine if the named [Attribute] is present in this set
    pub fn has<Q>(&self, key: &Q) -> bool
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let key = key.borrow();
        self.0.binary_search_by(|attr| key.cmp(attr.name.borrow()).reverse()).is_ok()
    }

    /// Get the [AttributeValue] associated with the named [Attribute]
    pub fn get_any<Q>(&self, key: &Q) -> Option<&dyn AttributeValue>
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let key = key.borrow();
        match self.0.binary_search_by(|attr| key.cmp(attr.name.borrow())) {
            Ok(index) => self.0[index].value.as_deref(),
            Err(_) => None,
        }
    }

    /// Get the [AttributeValue] associated with the named [Attribute]
    pub fn get_any_mut<Q>(&mut self, key: &Q) -> Option<&mut dyn AttributeValue>
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let key = key.borrow();
        match self.0.binary_search_by(|attr| key.cmp(attr.name.borrow())) {
            Ok(index) => self.0[index].value.as_deref_mut(),
            Err(_) => None,
        }
    }

    /// Get the value associated with the named [Attribute] as a value of type `V`, or `None`.
    pub fn get<V, Q>(&self, key: &Q) -> Option<&V>
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
        V: AttributeValue,
    {
        self.get_any(key).and_then(|v| v.downcast_ref::<V>())
    }

    /// Get the value associated with the named [Attribute] as a value of type `V`, or `None`.
    pub fn get_mut<V, Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        Symbol: Borrow<Q>,
        Q: Ord + ?Sized,
        V: AttributeValue,
    {
        self.get_any_mut(key).and_then(|v| v.downcast_mut::<V>())
    }

    /// Iterate over each [Attribute] in this set
    pub fn iter(&self) -> impl Iterator<Item = &Attribute> + '_ {
        self.0.iter()
    }
}

/// An [Attribute] associates some data with a well-known identifier (name).
///
/// Attributes are used for representing metadata that helps guide compilation,
/// but which is not part of the code itself. For example, `cfg` flags in Rust
/// are an example of something which you could represent using an [Attribute].
/// They can also be used to store documentation, source locations, and more.
#[derive(Debug)]
pub struct Attribute {
    /// The name of this attribute
    pub name: Symbol,
    /// The value associated with this attribute
    pub value: Option<Box<dyn AttributeValue>>,
}
impl Attribute {
    pub fn new(name: impl Into<Symbol>, value: Option<impl AttributeValue>) -> Self {
        Self {
            name: name.into(),
            value: value.map(|v| Box::new(v) as Box<dyn AttributeValue>),
        }
    }

    pub fn value(&self) -> Option<&dyn AttributeValue> {
        self.value.as_deref()
    }

    pub fn value_as<V>(&self) -> Option<&V>
    where
        V: AttributeValue,
    {
        match self.value.as_deref() {
            Some(value) => value.downcast_ref::<V>(),
            None => None,
        }
    }
}
impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.value.as_deref() {
            None => write!(f, "#[{}]", self.name.as_str()),
            Some(value) => write!(f, "#[{}({value})]", &self.name),
        }
    }
}

pub trait AttributeValue: Any + fmt::Debug + fmt::Display + 'static {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl dyn AttributeValue {
    pub fn is<T: AttributeValue>(&self) -> bool {
        self.as_any().is::<T>()
    }

    pub fn downcast_ref<T: AttributeValue>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    pub fn downcast_mut<T: AttributeValue>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

#[macro_export]
macro_rules! define_attr_type {
    ($T:ty) => {
        impl $crate::AttributeValue for $T {
            #[inline(always)]
            fn as_any(&self) -> &dyn core::any::Any {
                self as &dyn core::any::Any
            }

            #[inline(always)]
            fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
                self as &mut dyn core::any::Any
            }
        }
    };
}

define_attr_type!(bool);
define_attr_type!(isize);
define_attr_type!(Symbol);
define_attr_type!(super::Immediate);
define_attr_type!(super::Type);
