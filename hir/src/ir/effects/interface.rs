use smallvec::SmallVec;

use super::*;
use crate::{OperationRef, SymbolRef, ValueRef};

/// Marker trait for ops with recursive effects of a given [Effect] type.
///
/// Ops with recursive effects are considered to have any of the effects of operations nested within
/// its regions, in addition to any effects it declares on itself. Only when the operation and none
/// of its nested operations carry effects of the given type, can it be assumed that the operation
/// is free of that effect.
pub trait HasRecursiveEffects<T: Effect> {}

impl<T: HasRecursiveMemoryEffects> HasRecursiveEffects<MemoryEffect> for T {}

pub trait EffectOpInterface<T: Effect> {
    /// Return the set all of the operation's effects
    fn effects(&self) -> EffectIterator<T>;
    /// Returns true if this operation has no effects
    fn has_no_effect(&self) -> bool {
        self.effects().is_empty()
    }
    /// Return the set of effect instances that operate on the provided value
    fn effects_on_value(&self, value: ValueRef) -> ValueEffectIterator<T> {
        EffectIterator::for_value(self.effects(), value)
    }
    /// Return the set of effect instances that operate on the provided symbol
    fn effects_on_symbol(&self, symbol: SymbolRef) -> SymbolEffectIterator<T> {
        EffectIterator::for_symbol(self.effects(), symbol)
    }
    /// Return the set of effect instances that operate on the provided resource
    fn effects_on_resource<'a, 'b: 'a>(
        &self,
        resource: &'b dyn Resource,
    ) -> ResourceEffectIterator<'b, T> {
        EffectIterator::for_resource(self.effects(), resource)
    }
}

impl<T: Effect> dyn EffectOpInterface<T> {
    /// Return the set all of the operation's effects that correspond to effect type `T`
    pub fn effects_of_type<E>(&self) -> impl Iterator<Item = EffectInstance<T>> + '_
    where
        E: Effect,
    {
        self.effects().filter(|instance| (instance.effect() as &dyn Any).is::<E>())
    }

    /// Returns true if the operation exhibits the given effect.
    pub fn has_effect<E>(&self) -> bool
    where
        E: Any,
    {
        self.effects().any(|instance| (instance.effect() as &dyn Any).is::<E>())
    }

    /// Returns true if the operation only exhibits the given effect.
    pub fn only_has_effect<E>(&self) -> bool
    where
        E: Any,
    {
        let mut effects = self.effects();
        !effects.is_empty() && effects.all(|instance| (instance.effect() as &dyn Any).is::<E>())
    }
}

pub struct EffectIterator<T> {
    effects: smallvec::IntoIter<[EffectInstance<T>; 4]>,
}
impl<T> EffectIterator<T> {
    pub fn from_smallvec(effects: SmallVec<[EffectInstance<T>; 4]>) -> Self {
        Self {
            effects: effects.into_iter(),
        }
    }

    pub fn new(effects: impl IntoIterator<Item = EffectInstance<T>>) -> Self {
        let effects = effects.into_iter().collect::<SmallVec<[_; 4]>>();
        Self {
            effects: effects.into_iter(),
        }
    }

    pub const fn for_value(effects: Self, value: ValueRef) -> ValueEffectIterator<T> {
        ValueEffectIterator {
            iter: effects,
            value,
        }
    }

    pub const fn for_symbol(effects: Self, symbol: SymbolRef) -> SymbolEffectIterator<T> {
        SymbolEffectIterator {
            iter: effects,
            symbol,
        }
    }

    pub const fn for_resource(
        effects: Self,
        resource: &dyn Resource,
    ) -> ResourceEffectIterator<'_, T> {
        ResourceEffectIterator {
            iter: effects,
            resource,
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[EffectInstance<T>] {
        self.effects.as_slice()
    }
}
impl<T> core::iter::FusedIterator for EffectIterator<T> {}
impl<T> ExactSizeIterator for EffectIterator<T> {
    fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    fn len(&self) -> usize {
        self.effects.len()
    }
}
impl<T> Iterator for EffectIterator<T> {
    type Item = EffectInstance<T>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.effects.next()
    }
}

pub struct ValueEffectIterator<T> {
    iter: EffectIterator<T>,
    value: ValueRef,
}
impl<T> core::iter::FusedIterator for ValueEffectIterator<T> {}
impl<T> Iterator for ValueEffectIterator<T> {
    type Item = EffectInstance<T>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(instance) = self.iter.next() {
            if instance.value().is_some_and(|v| v == self.value) {
                return Some(instance);
            }
        }

        None
    }
}

pub struct SymbolEffectIterator<T> {
    iter: EffectIterator<T>,
    symbol: SymbolRef,
}
impl<T> core::iter::FusedIterator for SymbolEffectIterator<T> {}
impl<T> Iterator for SymbolEffectIterator<T> {
    type Item = EffectInstance<T>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(instance) = self.iter.next() {
            if instance.symbol().is_some_and(|s| s == self.symbol) {
                return Some(instance);
            }
        }

        None
    }
}

pub struct ResourceEffectIterator<'a, T> {
    iter: EffectIterator<T>,
    resource: &'a dyn Resource,
}
impl<T> core::iter::FusedIterator for ResourceEffectIterator<'_, T> {}
impl<T> Iterator for ResourceEffectIterator<'_, T> {
    type Item = EffectInstance<T>;

    fn next(&mut self) -> Option<Self::Item> {
        #[allow(clippy::while_let_on_iterator)]
        while let Some(instance) = self.iter.next() {
            if instance.resource().dyn_eq(self.resource) {
                return Some(instance);
            }
        }

        None
    }
}

/// An iterator over the recursive effects of an [crate::Operation].
///
/// The value produced by the iterator is `(OperationRef, Option<EffectInterface<T>>)`, where the
/// operation reference is the effecting op, and the second element is the identified effect:
///
/// * `Some` represents an effect on the operation or one of its nested operations
/// * `None` indicates that we have identified that the given operation has unknown effects, and
///   thus the entire operation could have unknown effects.
///
/// Note that in the case of discovering that an operation has unknown effects, the iterator can
/// continue to visit all effects recursively - it is up to the caller to stop iteration if the
/// presence of unknown effects makes further search wasteful.
pub struct RecursiveEffectIterator<T> {
    buffer: crate::adt::SmallDeque<(OperationRef, Option<EffectInstance<T>>), 2>,
    effecting_ops: SmallVec<[OperationRef; 4]>,
}

impl<T: Effect> RecursiveEffectIterator<T> {
    /// Iterate over the recursive effects of `op`
    pub fn new(op: OperationRef) -> Self {
        Self {
            buffer: Default::default(),
            effecting_ops: SmallVec::from_iter([op]),
        }
    }
}

impl<T: Effect> core::iter::FusedIterator for RecursiveEffectIterator<T> {}

impl<T: Effect> Iterator for RecursiveEffectIterator<T> {
    type Item = (OperationRef, Option<EffectInstance<T>>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(next) = self.buffer.pop_front() {
                return Some(next);
            }

            if let Some(op) = self.effecting_ops.pop() {
                let operation = op.borrow();

                let has_recursive_effects = operation.implements::<dyn HasRecursiveEffects<T>>();
                if has_recursive_effects {
                    for region in operation.regions() {
                        for block in region.body() {
                            let mut next = block.body().front().as_pointer();
                            while let Some(nested) = next.take() {
                                next = nested.next();
                                self.effecting_ops.push(nested);
                            }
                        }
                    }
                }

                if let Some(effect_interface) = operation.as_trait::<dyn EffectOpInterface<T>>() {
                    self.buffer.extend(effect_interface.effects().map(|eff| (op, Some(eff))));
                } else if !has_recursive_effects {
                    // The operation does not have recursive memory effects or implement
                    // EffectOpInterface, so its effects are unknown.
                    self.buffer.push_back((op, None));
                }
            } else {
                break;
            }
        }

        None
    }
}
