mod instance;
mod interface;
mod memory;
mod speculation;

use core::{any::Any, fmt};
use std::iter::FusedIterator;

pub use self::{instance::EffectInstance, interface::*, memory::*, speculation::*};
use crate::DynPartialEq;

pub trait Effect: Any + fmt::Debug {}

pub trait Resource: Any + DynPartialEq + fmt::Debug {
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
