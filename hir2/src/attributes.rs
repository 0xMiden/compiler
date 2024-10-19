mod call_conv;
mod overflow;
mod visibility;

use alloc::collections::BTreeMap;
use core::{any::Any, borrow::Borrow, fmt};

pub use self::{call_conv::CallConv, overflow::Overflow, visibility::Visibility};
use crate::interner::Symbol;

pub mod markers {
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
#[derive(Debug, Default, Hash)]
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
        self.set(Attribute {
            name: name.into(),
            value: value.map(|v| Box::new(v) as Box<dyn AttributeValue>),
        });
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
    pub fn remove(&mut self, name: impl Into<Symbol>) {
        let name = name.into();
        match self.0.binary_search_by_key(&name, |attr| attr.name) {
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
    pub fn has(&self, key: impl Into<Symbol>) -> bool {
        let key = key.into();
        self.0.binary_search_by_key(&key, |attr| attr.name).is_ok()
    }

    /// Get the [AttributeValue] associated with the named [Attribute]
    pub fn get_any(&self, key: impl Into<Symbol>) -> Option<&dyn AttributeValue> {
        let key = key.into();
        match self.0.binary_search_by_key(&key, |attr| attr.name) {
            Ok(index) => self.0[index].value.as_deref(),
            Err(_) => None,
        }
    }

    /// Get the [AttributeValue] associated with the named [Attribute]
    pub fn get_any_mut(&mut self, key: impl Into<Symbol>) -> Option<&mut dyn AttributeValue> {
        let key = key.into();
        match self.0.binary_search_by_key(&key, |attr| attr.name) {
            Ok(index) => self.0[index].value.as_deref_mut(),
            Err(_) => None,
        }
    }

    /// Get the value associated with the named [Attribute] as a value of type `V`, or `None`.
    pub fn get<V>(&self, key: impl Into<Symbol>) -> Option<&V>
    where
        V: AttributeValue,
    {
        self.get_any(key).and_then(|v| v.downcast_ref::<V>())
    }

    /// Get the value associated with the named [Attribute] as a mutable value of type `V`, or
    /// `None`.
    pub fn get_mut<V>(&mut self, key: impl Into<Symbol>) -> Option<&mut V>
    where
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
#[derive(Debug, Hash)]
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
        match self.value.as_deref().map(|v| v.render()) {
            None => write!(f, "#[{}]", self.name.as_str()),
            Some(value) => write!(f, "#[{}({value})]", &self.name),
        }
    }
}

pub trait AttributeValue:
    Any + fmt::Debug + crate::formatter::PrettyPrint + crate::DynHash + 'static
{
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn clone_value(&self) -> Box<dyn AttributeValue>;
}

impl dyn AttributeValue {
    pub fn is<T: AttributeValue>(&self) -> bool {
        self.as_any().is::<T>()
    }

    pub fn downcast<T: AttributeValue>(self: Box<Self>) -> Result<Box<T>, Box<Self>> {
        if self.is::<T>() {
            let ptr = Box::into_raw(self);
            Ok(unsafe { Box::from_raw(ptr.cast()) })
        } else {
            Err(self)
        }
    }

    pub fn downcast_ref<T: AttributeValue>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    pub fn downcast_mut<T: AttributeValue>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

impl core::hash::Hash for dyn AttributeValue {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        use crate::DynHash;

        let hashable = self as &dyn DynHash;
        hashable.dyn_hash(state);
    }
}

#[derive(Clone)]
pub struct SetAttr<K> {
    values: Vec<K>,
}
impl<K> Default for SetAttr<K> {
    fn default() -> Self {
        Self {
            values: Default::default(),
        }
    }
}
impl<K> SetAttr<K>
where
    K: Ord + Clone,
{
    pub fn insert(&mut self, key: K) -> bool {
        match self.values.binary_search_by(|k| key.cmp(k)) {
            Ok(index) => {
                self.values[index] = key;
                false
            }
            Err(index) => {
                self.values.insert(index, key);
                true
            }
        }
    }

    pub fn contains(&self, key: &K) -> bool {
        self.values.binary_search_by(|k| key.cmp(k)).is_ok()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, K> {
        self.values.iter()
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<K>
    where
        K: Borrow<Q>,
        Q: ?Sized + Ord,
    {
        match self.values.binary_search_by(|k| key.cmp(k.borrow())) {
            Ok(index) => Some(self.values.remove(index)),
            Err(_) => None,
        }
    }
}
impl<K> Eq for SetAttr<K> where K: Eq {}
impl<K> PartialEq for SetAttr<K>
where
    K: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}
impl<K> fmt::Debug for SetAttr<K>
where
    K: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.values.iter()).finish()
    }
}
impl<K> crate::formatter::PrettyPrint for SetAttr<K>
where
    K: crate::formatter::PrettyPrint,
{
    fn render(&self) -> crate::formatter::Document {
        todo!()
    }
}
impl<K> core::hash::Hash for SetAttr<K>
where
    K: core::hash::Hash,
{
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        <Vec<K> as core::hash::Hash>::hash(&self.values, state);
    }
}
impl<K> AttributeValue for SetAttr<K>
where
    K: fmt::Debug + crate::formatter::PrettyPrint + Clone + core::hash::Hash + 'static,
{
    #[inline(always)]
    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }

    #[inline(always)]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self as &mut dyn Any
    }

    #[inline]
    fn clone_value(&self) -> Box<dyn AttributeValue> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct DictAttr<K, V> {
    values: Vec<(K, V)>,
}
impl<K, V> Default for DictAttr<K, V> {
    fn default() -> Self {
        Self { values: vec![] }
    }
}
impl<K, V> DictAttr<K, V>
where
    K: Ord,
    V: Clone,
{
    pub fn insert(&mut self, key: K, value: V) {
        match self.values.binary_search_by(|(k, _)| key.cmp(k)) {
            Ok(index) => {
                self.values[index].1 = value;
            }
            Err(index) => {
                self.values.insert(index, (key, value));
            }
        }
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ?Sized + Ord,
    {
        self.values.binary_search_by(|(k, _)| key.cmp(k.borrow())).is_ok()
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Ord,
    {
        match self.values.binary_search_by(|(k, _)| key.cmp(k.borrow())) {
            Ok(index) => Some(&self.values[index].1),
            Err(_) => None,
        }
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Ord,
    {
        match self.values.binary_search_by(|(k, _)| key.cmp(k.borrow())) {
            Ok(index) => Some(self.values.remove(index).1),
            Err(_) => None,
        }
    }

    pub fn iter(&self) -> core::slice::Iter<'_, (K, V)> {
        self.values.iter()
    }
}
impl<K, V> Eq for DictAttr<K, V>
where
    K: Eq,
    V: Eq,
{
}
impl<K, V> PartialEq for DictAttr<K, V>
where
    K: PartialEq,
    V: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}
impl<K, V> fmt::Debug for DictAttr<K, V>
where
    K: fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.values.iter().map(|entry| (&entry.0, &entry.1)))
            .finish()
    }
}
impl<K, V> crate::formatter::PrettyPrint for DictAttr<K, V>
where
    K: crate::formatter::PrettyPrint,
    V: crate::formatter::PrettyPrint,
{
    fn render(&self) -> crate::formatter::Document {
        todo!()
    }
}
impl<K, V> core::hash::Hash for DictAttr<K, V>
where
    K: core::hash::Hash,
    V: core::hash::Hash,
{
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        <Vec<(K, V)> as core::hash::Hash>::hash(&self.values, state);
    }
}
impl<K, V> AttributeValue for DictAttr<K, V>
where
    K: fmt::Debug + crate::formatter::PrettyPrint + Clone + core::hash::Hash + 'static,
    V: fmt::Debug + crate::formatter::PrettyPrint + Clone + core::hash::Hash + 'static,
{
    #[inline(always)]
    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }

    #[inline(always)]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self as &mut dyn Any
    }

    #[inline]
    fn clone_value(&self) -> Box<dyn AttributeValue> {
        Box::new(self.clone())
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

            #[inline]
            fn clone_value(&self) -> Box<dyn $crate::AttributeValue> {
                Box::new(self.clone())
            }
        }
    };
}

define_attr_type!(bool);
define_attr_type!(u8);
define_attr_type!(i8);
define_attr_type!(u16);
define_attr_type!(i16);
define_attr_type!(u32);
define_attr_type!(core::num::NonZeroU32);
define_attr_type!(i32);
define_attr_type!(u64);
define_attr_type!(i64);
define_attr_type!(usize);
define_attr_type!(isize);
define_attr_type!(Symbol);
define_attr_type!(super::Immediate);
define_attr_type!(super::Type);
