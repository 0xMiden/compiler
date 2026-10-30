mod advice;
mod debug;
mod instance;
mod interface;
mod memory;
mod speculation;

use core::fmt;

pub use self::{
    advice::*, debug::*, instance::EffectInstance, interface::*, memory::*, speculation::*,
};
use crate::{DynPartialEq, any::AsAny, eq::PartialEqable};

pub trait Effect: AsAny + fmt::Debug {
    /// Returns true if `op` declares that its effects of this kind include those of the operations
    /// nested within its regions.
    ///
    /// **THE MARKER AN OP ACTUALLY REGISTERS IS THE CONCRETE ONE.**
    /// `#[operation(traits(HasRecursiveMemoryEffects))]` puts `dyn HasRecursiveMemoryEffects` in
    /// the op's trait table. `dyn HasRecursiveEffects<MemoryEffect>` is a different `TypeId` that
    /// no op registers, even though the blanket impl in [`interface`] makes the bound hold — so
    /// asking for the generic form answered `false` for every op in the compiler. This hook asks
    /// for the marker the op really carries.
    fn is_recursive_on(op: &crate::Operation) -> bool
    where
        Self: Sized;
}

pub trait Resource: AsAny + PartialEqable + DynPartialEq + fmt::Debug {
    fn name(&self) -> &'static str;
}

/// A conservative default resource kind
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DefaultResource;
impl Resource for DefaultResource {
    fn name(&self) -> &'static str {
        "default"
    }
}

/// An automatic allocation-scope resource that is valid in the context of a parent
/// AutomaticAllocationScope trait.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AutomaticAllocationScopeResource;
impl Resource for AutomaticAllocationScopeResource {
    fn name(&self) -> &'static str {
        "automatic-allocation-scope"
    }
}

/// An operation trait for ops that are always speculatable and have no memory effects
pub trait Pure: AlwaysSpeculatable + MemoryEffectOpInterface {}
