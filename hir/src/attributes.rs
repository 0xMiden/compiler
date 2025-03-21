mod call_conv;
mod overflow;
mod visibility;

use alloc::{boxed::Box, collections::BTreeMap, vec, vec::Vec};
use core::{any::Any, borrow::Borrow, fmt};

pub use self::{call_conv::CallConv, overflow::Overflow, visibility::Visibility};
use crate::{interner::Symbol, Immediate};

pub mod markers {
    use midenc_hir_symbol::symbols;

    use super::*;

    /// This attribute indicates that the decorated function is the entrypoint
    /// for its containing program, regardless of what module it is defined in.
    pub const ENTRYPOINT: Attribute = Attribute {
        name: symbols::Entrypoint,
        value: None,
        intrinsic: false,
    };
}

/// An [AttributeSet] is a uniqued collection of attributes associated with some IR entity
#[derive(Debug, Default, Clone)]
pub struct AttributeSet(Vec<Attribute>);
impl FromIterator<Attribute> for AttributeSet {
    fn from_iter<T>(attrs: T) -> Self
    where
        T: IntoIterator<Item = Attribute>,
    {
        let mut map = BTreeMap::default();
        for attr in attrs.into_iter() {
            map.insert(attr.name, (attr.value, attr.intrinsic));
        }
        Self(
            map.into_iter()
                .map(|(name, (value, intrinsic))| Attribute {
                    name,
                    value,
                    intrinsic,
                })
                .collect(),
        )
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
        Self(
            map.into_iter()
                .map(|(name, value)| Attribute {
                    name,
                    value,
                    intrinsic: false,
                })
                .collect(),
        )
    }
}
impl AttributeSet {
    /// Get a new, empty [AttributeSet]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Insert a new [Attribute] in this set by `name` and `value`
    pub fn insert(&mut self, name: impl Into<Symbol>, value: Option<impl AttributeValue>) {
        self.set(Attribute {
            name: name.into(),
            value: value.map(|v| Box::new(v) as Box<dyn AttributeValue>),
            intrinsic: false,
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

    pub fn mark_intrinsic(&mut self, key: impl Into<Symbol>) {
        let key = key.into();
        if let Ok(index) = self.0.binary_search_by_key(&key, |attr| attr.name) {
            self.0[index].intrinsic = true;
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

impl Eq for AttributeSet {}
impl PartialEq for AttributeSet {
    fn eq(&self, other: &Self) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }

        for attr in self.0.iter() {
            if !other.has(attr.name) {
                return false;
            }

            let other_value = other.get_any(attr.name);
            if attr.value() != other_value {
                return false;
            }
        }

        true
    }
}

impl core::hash::Hash for AttributeSet {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.len().hash(state);

        for attr in self.0.iter() {
            attr.hash(state);
        }
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
    /// This attribute represents an intrinsic property of an operation
    pub intrinsic: bool,
}
impl Clone for Attribute {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            value: self.value.as_ref().map(|v| v.clone_value()),
            intrinsic: self.intrinsic,
        }
    }
}
impl Attribute {
    pub fn new(name: impl Into<Symbol>, value: Option<impl AttributeValue>) -> Self {
        Self {
            name: name.into(),
            value: value.map(|v| Box::new(v) as Box<dyn AttributeValue>),
            intrinsic: false,
        }
    }

    pub fn intrinsic(name: impl Into<Symbol>, value: Option<impl AttributeValue>) -> Self {
        Self {
            name: name.into(),
            value: value.map(|v| Box::new(v) as Box<dyn AttributeValue>),
            intrinsic: true,
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

pub trait AttributeValue:
    Any + fmt::Debug + crate::AttrPrinter + crate::DynPartialEq + crate::DynHash + 'static
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

    pub fn as_bool(&self) -> Option<bool> {
        if let Some(imm) = self.downcast_ref::<Immediate>() {
            imm.as_bool()
        } else {
            self.downcast_ref::<bool>().copied()
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        if let Some(imm) = self.downcast_ref::<Immediate>() {
            imm.as_u32()
        } else {
            self.downcast_ref::<u32>().copied()
        }
    }

    pub fn as_immediate(&self) -> Option<Immediate> {
        self.downcast_ref::<Immediate>().copied()
    }
}

impl core::hash::Hash for dyn AttributeValue {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        use crate::DynHash;

        let hashable = self as &dyn DynHash;
        hashable.dyn_hash(state);
    }
}

impl Eq for dyn AttributeValue {}
impl PartialEq for dyn AttributeValue {
    fn eq(&self, other: &Self) -> bool {
        use crate::DynPartialEq;

        let partial_eqable = self as &dyn DynPartialEq;
        partial_eqable.dyn_eq(other as &dyn DynPartialEq)
    }
}

#[derive(Clone)]
pub struct ArrayAttr<T> {
    values: Vec<T>,
}
impl<T> Default for ArrayAttr<T> {
    fn default() -> Self {
        Self {
            values: Default::default(),
        }
    }
}
impl<T> FromIterator<T> for ArrayAttr<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            values: Vec::<T>::from_iter(iter),
        }
    }
}
impl<T> ArrayAttr<T> {
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.values.iter()
    }

    pub fn push(&mut self, value: T) {
        self.values.push(value);
    }

    pub fn remove(&mut self, index: usize) -> T {
        self.values.remove(index)
    }
}
impl<T> ArrayAttr<T>
where
    T: Eq,
{
    pub fn contains(&self, value: &T) -> bool {
        self.values.contains(value)
    }
}
impl<T> Eq for ArrayAttr<T> where T: Eq {}
impl<T> PartialEq for ArrayAttr<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}
impl<T> fmt::Debug for ArrayAttr<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.values.iter()).finish()
    }
}
impl<T> crate::formatter::PrettyPrint for ArrayAttr<T>
where
    T: crate::formatter::PrettyPrint,
{
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        let entries = self.values.iter().fold(Document::Empty, |acc, v| match acc {
            Document::Empty => v.render(),
            _ => acc + const_text(", ") + v.render(),
        });
        if self.values.is_empty() {
            const_text("[]")
        } else {
            const_text("[") + entries + const_text("]")
        }
    }
}
impl<T> core::hash::Hash for ArrayAttr<T>
where
    T: core::hash::Hash,
{
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        <Vec<T> as core::hash::Hash>::hash(&self.values, state);
    }
}
impl<T> AttributeValue for ArrayAttr<T>
where
    T: fmt::Debug + crate::formatter::PrettyPrint + Clone + Eq + core::hash::Hash + 'static,
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
        use crate::formatter::*;

        let entries = self.values.iter().fold(Document::Empty, |acc, k| match acc {
            Document::Empty => k.render(),
            _ => acc + const_text(", ") + k.render(),
        });
        if self.values.is_empty() {
            const_text("{}")
        } else {
            const_text("{") + entries + const_text("}")
        }
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
    K: fmt::Debug + crate::formatter::PrettyPrint + Clone + Eq + core::hash::Hash + 'static,
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
        use crate::formatter::*;

        let entries = self.values.iter().fold(Document::Empty, |acc, (k, v)| match acc {
            Document::Empty => k.render() + const_text(" = ") + v.render(),
            _ => acc + const_text(", ") + k.render() + const_text(" = ") + v.render(),
        });
        if self.values.is_empty() {
            const_text("{}")
        } else {
            const_text("{") + entries + const_text("}")
        }
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
    K: fmt::Debug + crate::formatter::PrettyPrint + Clone + Eq + core::hash::Hash + 'static,
    V: fmt::Debug + crate::formatter::PrettyPrint + Clone + Eq + core::hash::Hash + 'static,
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
            fn clone_value(&self) -> ::alloc::boxed::Box<dyn $crate::AttributeValue> {
                ::alloc::boxed::Box::new(self.clone())
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
