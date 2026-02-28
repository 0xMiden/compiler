use super::{Attribute, AttributeRef};
use crate::{
    Entity, EntityList, EntityListCursor, EntityListCursorMut, EntityListItem, EntityMap,
    EntityMapCursor, EntityMapCursorMut, EntityMapItem, EntityParent, EntityRef, EntityWithKey,
    EntityWithParent, Operation, UnsafeIntrusiveEntityRef, entity::UnsafeIntrusiveMapEntityRef,
    interner,
};

pub type NamedAttributeList = EntityList<NamedAttribute>;
pub type NamedAttributeListRef = UnsafeIntrusiveEntityRef<NamedAttribute>;
pub type NamedAttributeCursor<'a> = EntityListCursor<'a, NamedAttribute>;
pub type NamedAttributeCursorMut<'a> = EntityListCursorMut<'a, NamedAttribute>;

pub type AttributeDict = EntityMap<NamedAttribute>;
pub type AttributeDictEntryRef = UnsafeIntrusiveMapEntityRef<NamedAttribute>;
pub type AttributeDictCursor<'a> = EntityMapCursor<'a, NamedAttribute>;
pub type AttributeDictCursorMut<'a> = EntityMapCursorMut<'a, NamedAttribute>;

impl PartialEq for AttributeDictEntryRef {
    fn eq(&self, other: &Self) -> bool {
        if Self::ptr_eq(self, other) {
            true
        } else {
            self.borrow() == other.borrow()
        }
    }
}

/// A [NamedAttribute] associates an [Attribute] with a well-known identifier (name).
///
/// Named attributes are used for representing metadata that helps guide compilation, but which is
/// not part of the code itself. For example, `cfg` flags in Rust are an example of something which
/// you could represent using a [NamedAttribute]. They can also be used to store documentation,
/// source locations, and more.
#[derive(Debug, Copy, Clone)]
pub struct NamedAttribute {
    /// The name of this attribute
    pub name: interner::Symbol,
    /// The value associated with this attribute
    pub value: AttributeRef,
}

impl NamedAttribute {
    pub fn new(name: impl Into<interner::Symbol>, value: AttributeRef) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }

    pub fn value(&self) -> EntityRef<'_, dyn Attribute> {
        self.value.borrow()
    }
}

impl Eq for NamedAttribute {}
impl PartialEq for NamedAttribute {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.value.borrow().dyn_eq(&other.value.borrow())
    }
}
impl core::hash::Hash for NamedAttribute {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.value.borrow().hash(state);
    }
}

impl Entity for NamedAttribute {}
impl EntityListItem for NamedAttribute {}
impl EntityMapItem for NamedAttribute {}
impl EntityWithKey for NamedAttribute {
    type Key = interner::Symbol;
    type Value = AttributeRef;

    #[inline(always)]
    fn key(&self) -> Self::Key {
        self.name
    }

    #[inline(always)]
    fn value(&self) -> &Self::Value {
        &self.value
    }
}

/// A wrapper around a [NamedAttribute] stored in the attribute dictionary of an [Operation]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct OpAttribute(NamedAttribute);

impl OpAttribute {
    #[inline(always)]
    pub const fn as_named_attribute(&self) -> &NamedAttribute {
        &self.0
    }
}

impl PartialEq for UnsafeIntrusiveMapEntityRef<OpAttribute> {
    fn eq(&self, other: &Self) -> bool {
        if Self::ptr_eq(self, other) {
            true
        } else {
            self.borrow() == other.borrow()
        }
    }
}

impl AsRef<NamedAttribute> for OpAttribute {
    #[inline(always)]
    fn as_ref(&self) -> &NamedAttribute {
        &self.0
    }
}

impl AsMut<NamedAttribute> for OpAttribute {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut NamedAttribute {
        &mut self.0
    }
}

impl From<NamedAttribute> for OpAttribute {
    fn from(value: NamedAttribute) -> Self {
        Self(value)
    }
}

impl From<OpAttribute> for NamedAttribute {
    fn from(value: OpAttribute) -> Self {
        value.0
    }
}

impl Entity for OpAttribute {}
impl EntityMapItem for OpAttribute {}
impl EntityWithKey for OpAttribute {
    type Key = interner::Symbol;
    type Value = AttributeRef;

    #[inline(always)]
    fn key(&self) -> Self::Key {
        self.0.name
    }

    #[inline(always)]
    fn value(&self) -> &Self::Value {
        &self.0.value
    }
}

impl EntityWithParent for OpAttribute {
    type Parent = Operation;
}

impl EntityParent<OpAttribute> for Operation {
    fn offset() -> usize {
        core::mem::offset_of!(Operation, attrs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialects::builtin::attributes::ImmediateAttr, testing::Test};

    #[test]
    fn named_attribute_equality() {
        let test = Test::default();

        let zero = test.context_rc().create_attribute::<ImmediateAttr, _>(0u32);
        let one = test.context_rc().create_attribute::<ImmediateAttr, _>(1u32);
        let a = NamedAttribute::new("a", zero);
        let a2 = NamedAttribute::new("a", zero);
        let a3 = NamedAttribute::new("a", one);
        let b = NamedAttribute::new("b", zero);

        assert_eq!(&a, &a);
        assert_eq!(&a, &a2);
        assert_ne!(&a, &a3);
        assert_ne!(&a, &b);
    }
}
