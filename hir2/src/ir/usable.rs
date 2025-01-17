use super::{
    entity::EntityIter, EntityCursor, EntityCursorMut, EntityList, UnsafeIntrusiveEntityRef,
};

/// The [Usable] trait is implemented for IR entities which are _defined_ and _used_, and as a
/// result, require a data structure called the _use-def list_.
///
/// A _definition_ of an IR entity, is a unique instantiation of that entity, the result of which
/// is different from all other definitions, even if the data associated with that definition is
/// the same as another definition. For example, SSA values are defined as either block arguments
/// or operation results, and a given value can only be defined once.
///
/// A _use_ represents a unique reference to a _definition_ of some IR entity. Each use is unique,
/// and can be used to obtain not only the _user_ of the reference, but the location of that use
/// within the user. Uses are tracked in a _use list_, also called the _use-def list_, which
/// associates all uses to the definition, or _def_, that they reference. For example, operations
/// in HIR _use_ SSA values defined previously in the program.
///
/// A _user_ does not have to be of the same IR type as the _definition_, and the type representing
/// the _use_ is typically different than both, and represents the type of relationship between the
/// two. For example, an `OpOperand` represents a single use of a `Value` by an `Op`. The entity
/// being defined is a `Value`, the entity using that definition is an `Op`, and the data associated
/// with each use is represented by `OpOperand`.
pub trait Usable {
    /// The type associated with each unique use, e.g. `OpOperand`
    type Use;

    /// Get a list of uses of this definition
    fn uses(&self) -> &EntityList<Self::Use>;
    /// Get a mutable list of uses of this definition
    fn uses_mut(&mut self) -> &mut EntityList<Self::Use>;

    /// Returns true if this definition is used
    #[inline]
    fn is_used(&self) -> bool {
        !self.uses().is_empty()
    }
    /// Get an iterator over the uses of this definition
    #[inline]
    fn iter_uses(&self) -> EntityIter<'_, Self::Use> {
        self.uses().iter()
    }
    /// Get a cursor positioned on the first use of this definition, or the null cursor if unused.
    fn first_use(&self) -> EntityCursor<'_, Self::Use> {
        self.uses().front()
    }
    /// Get a mutable cursor positioned on the first use of this definition, or the null cursor if
    /// unused.
    #[inline]
    fn first_use_mut(&mut self) -> EntityCursorMut<'_, Self::Use> {
        self.uses_mut().front_mut()
    }
    /// Add `user` to the set of uses of this definition
    fn insert_use(&mut self, user: UnsafeIntrusiveEntityRef<Self::Use>) {
        self.uses_mut().push_back(user);
    }
}
