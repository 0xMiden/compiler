use smallvec::SmallVec;

use super::*;
use crate::{SymbolRef, ValueRef};

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
impl<T> FusedIterator for EffectIterator<T> {}
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
impl<T> FusedIterator for ValueEffectIterator<T> {}
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
impl<T> FusedIterator for SymbolEffectIterator<T> {}
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
impl<T> FusedIterator for ResourceEffectIterator<'_, T> {}
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
