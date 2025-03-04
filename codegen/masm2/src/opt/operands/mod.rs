mod context;
mod solver;
mod stack;
mod tactics;
#[cfg(test)]
mod testing;

use core::{fmt, num::NonZeroU8};

use midenc_hir2 as hir;

pub use self::solver::{OperandMovementConstraintSolver, SolverError};
use self::{context::SolverContext, stack::Stack};

/// This represents a specific action that should be taken by
/// the code generator with regard to an operand on the stack.
///
/// The output of the optimizer is a sequence of these actions,
/// the effect of which is to place all of the current instruction's
/// operands exactly where they need to be, just when they are
/// needed.
#[derive(Debug, Copy, Clone)]
pub enum Action {
    /// Copy the operand at the given index to the top of the stack
    Copy(u8),
    /// Swap the operand at the given index with the one on top of the stack
    Swap(u8),
    /// Move the operand at the given index to the top of the stack
    MoveUp(u8),
    /// Move the operand at the top of the stack to the given index
    MoveDown(u8),
}

/// This is a [midenc_hir::Value], but with a modified encoding that lets
/// us uniquely identify aliases of a value on the operand stack during
/// analysis.
///
/// Aliases of a value are treated as unique values for purposes of operand
/// stack management, but are associated with multiple copies of a value
/// on the stack.
#[derive(Copy, Clone)]
pub struct ValueOrAlias {
    value: hir::ValueRef,
    value_id: hir::ValueId,
    id: u8,
}
impl Eq for ValueOrAlias {}
impl PartialEq for ValueOrAlias {
    fn eq(&self, other: &Self) -> bool {
        self.value_id == other.value_id && self.id == other.id
    }
}
impl core::hash::Hash for ValueOrAlias {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.value_id.hash(state);
        self.id.hash(state);
    }
}
impl ValueOrAlias {
    /// Create a new [ValueOrAlias] from the given [hir::ValueRef]
    pub fn new(value: hir::ValueRef) -> Self {
        let value_id = value.borrow().id();
        Self {
            value,
            value_id,
            id: 0,
        }
    }

    /// Create an aliased copy of this value, using `id` to uniquely identify the alias.
    ///
    /// NOTE: You must ensure that each alias of the same value gets a unique identifier,
    /// or you may observe strange behavior due to two aliases that should be distinct,
    /// being treated as if they have the same identity.
    pub fn copy(mut self, id: NonZeroU8) -> Self {
        self.id = id.get();
        self
    }

    /// Get an un-aliased copy of this value
    pub fn unaliased(mut self) -> Self {
        self.id = 0;
        self
    }

    /// Convert this value into an alias, using `id` to uniquely identify the alias.
    ///
    /// NOTE: You must ensure that each alias of the same value gets a unique identifier,
    /// or you may observe strange behavior due to two aliases that should be distinct,
    /// being treated as if they have the same identity.
    pub fn set_alias(&mut self, id: NonZeroU8) {
        self.id = id.get();
    }

    /// Get the underlying [hir::ValueRef]
    pub fn value(self) -> hir::ValueRef {
        self.value
    }

    /// Get the unique alias identifier for this value, if this value is an alias
    pub fn alias(self) -> Option<NonZeroU8> {
        NonZeroU8::new(self.id)
    }

    /// Get the unique alias identifier for this value, if this value is an alias
    pub fn unwrap_alias(self) -> NonZeroU8 {
        NonZeroU8::new(self.id).unwrap_or_else(|| panic!("expected {self:?} to be an alias"))
    }

    /// Returns true if this value is an alias
    pub fn is_alias(&self) -> bool {
        self.id != 0
    }
}
impl Ord for ValueOrAlias {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.value_id.cmp(&other.value_id).then(self.id.cmp(&other.id))
    }
}
impl PartialEq<hir::ValueRef> for ValueOrAlias {
    fn eq(&self, other: &hir::ValueRef) -> bool {
        &self.value == other
    }
}
impl PartialOrd for ValueOrAlias {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl From<hir::ValueRef> for ValueOrAlias {
    #[inline]
    fn from(value: hir::ValueRef) -> Self {
        Self::new(value)
    }
}
impl From<ValueOrAlias> for hir::ValueRef {
    #[inline]
    fn from(value: ValueOrAlias) -> Self {
        value.value
    }
}
impl fmt::Debug for ValueOrAlias {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.alias() {
            None => write!(f, "{}", &self.value_id),
            Some(alias) => write!(f, "{}.{alias}", &self.value_id),
        }
    }
}

/// This is an simple representation of an operand on the operand stack
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Operand {
    /// The position of this operand on the corresponding stack
    pub pos: u8,
    /// The value this operand corresponds to
    pub value: ValueOrAlias,
}
impl From<(usize, ValueOrAlias)> for Operand {
    #[inline(always)]
    fn from(pair: (usize, ValueOrAlias)) -> Self {
        Self {
            pos: pair.0 as u8,
            value: pair.1,
        }
    }
}
impl PartialEq<ValueOrAlias> for Operand {
    #[inline(always)]
    fn eq(&self, other: &ValueOrAlias) -> bool {
        self.value.eq(other)
    }
}
