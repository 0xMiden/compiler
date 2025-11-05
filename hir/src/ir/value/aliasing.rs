use core::{fmt, num::NonZeroU8};

use super::*;

/// This is a essentialy a [ValueRef], but with a modified encoding that lets us uniquely identify
/// aliases of a value on the operand stack during analysis.
///
/// Aliases of a value are treated as unique values for purposes of operand stack management, but
/// are associated with multiple copies of a value on the stack.
#[derive(Copy, Clone)]
pub struct ValueOrAlias {
    /// The SSA value of this operand
    value: ValueRef,
    /// To avoid unnecessary borrowing of `value`, we cache the ValueId here
    value_id: ValueId,
    /// When an SSA value is used multiple times by an instruction, each use must be accounted for
    /// on the operand stack as a unique value. The alias identifier is usually generated from a
    /// counter of the unique instances of the value, but can be any unique integer value.
    alias_id: u8,
}

impl ValueOrAlias {
    /// Create a new [ValueOrAlias] from the given [ValueRef]
    pub fn new(value: ValueRef) -> Self {
        let value_id = value.borrow().id();
        Self {
            value,
            value_id,
            alias_id: 0,
        }
    }

    /// Gets the effective size of this type on the Miden operand stack
    pub fn stack_size(&self) -> usize {
        self.value.borrow().ty().size_in_felts()
    }

    /// Create an aliased copy of this value, using `id` to uniquely identify the alias.
    ///
    /// NOTE: You must ensure that each alias of the same value gets a unique identifier,
    /// or you may observe strange behavior due to two aliases that should be distinct,
    /// being treated as if they have the same identity.
    pub fn copy(mut self, id: NonZeroU8) -> Self {
        self.alias_id = id.get();
        self
    }

    /// Get an un-aliased copy of this value
    pub fn unaliased(mut self) -> Self {
        self.alias_id = 0;
        self
    }

    /// Convert this value into an alias, using `id` to uniquely identify the alias.
    ///
    /// NOTE: You must ensure that each alias of the same value gets a unique identifier,
    /// or you may observe strange behavior due to two aliases that should be distinct,
    /// being treated as if they have the same identity.
    pub fn set_alias(&mut self, id: NonZeroU8) {
        self.alias_id = id.get();
    }

    /// Get the underlying [ValueRef]
    pub fn value(self) -> ValueRef {
        self.value
    }

    /// Borrow the underlying [Value]
    pub fn borrow_value(&self) -> EntityRef<'_, dyn Value> {
        self.value.borrow()
    }

    /// Get the unique alias identifier for this value, if this value is an alias
    pub fn alias(self) -> Option<NonZeroU8> {
        NonZeroU8::new(self.alias_id)
    }

    /// Get the unique alias identifier for this value, if this value is an alias
    pub fn unwrap_alias(self) -> NonZeroU8 {
        NonZeroU8::new(self.alias_id).unwrap_or_else(|| panic!("expected {self:?} to be an alias"))
    }

    /// Returns true if this value is an alias
    pub fn is_alias(&self) -> bool {
        self.alias_id != 0
    }
}

impl core::borrow::Borrow<ValueRef> for ValueOrAlias {
    #[inline(always)]
    fn borrow(&self) -> &ValueRef {
        &self.value
    }
}

impl core::borrow::Borrow<ValueRef> for &ValueOrAlias {
    #[inline(always)]
    fn borrow(&self) -> &ValueRef {
        &self.value
    }
}

impl core::borrow::Borrow<ValueRef> for &mut ValueOrAlias {
    #[inline(always)]
    fn borrow(&self) -> &ValueRef {
        &self.value
    }
}

impl Eq for ValueOrAlias {}

impl PartialEq for ValueOrAlias {
    fn eq(&self, other: &Self) -> bool {
        self.value_id == other.value_id && self.alias_id == other.alias_id
    }
}

impl core::hash::Hash for ValueOrAlias {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.value_id.hash(state);
        self.alias_id.hash(state);
    }
}

impl Ord for ValueOrAlias {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.value_id.cmp(&other.value_id).then(self.alias_id.cmp(&other.alias_id))
    }
}

impl PartialEq<ValueRef> for ValueOrAlias {
    fn eq(&self, other: &ValueRef) -> bool {
        &self.value == other
    }
}

impl PartialOrd for ValueOrAlias {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl From<ValueRef> for ValueOrAlias {
    #[inline]
    fn from(value: ValueRef) -> Self {
        Self::new(value)
    }
}

impl From<ValueOrAlias> for ValueRef {
    #[inline]
    fn from(value: ValueOrAlias) -> Self {
        value.value
    }
}

impl fmt::Display for ValueOrAlias {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.alias() {
            None => write!(f, "{}", &self.value_id),
            Some(alias) => write!(f, "{}.{alias}", &self.value_id),
        }
    }
}

impl fmt::Debug for ValueOrAlias {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
