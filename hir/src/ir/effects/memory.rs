use super::*;

/// Marker trait for ops with recursive memory effects, i.e. the effects of the operation includes
/// the effects of operations nested within its regions. If the operation does not implement any
/// effect markers, e.g. `MemoryWrite`, then it can be assumed to have no memory effects itself.
pub trait HasRecursiveMemoryEffects {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MemoryEffect {
    /// The following effect indicates that the operation reads from some resource.
    ///
    /// A 'read' effect implies only dereferencing of the resource, and not any visible mutation.
    Read,
    /// The following effect indicates that the operation writes to some resource.
    ///
    /// A 'write' effect implies only mutating a resource, and not any visible dereference or read.
    Write,
    /// The following effect indicates that the operation allocates from some resource.
    ///
    /// An 'allocate' effect implies only allocation of the resource, and not any visible mutation or
    /// dereference.
    Allocate,
    /// The following effect indicates that the operation frees some resource that has been
    /// allocated.
    ///
    /// An 'allocate' effect implies only de-allocation of the resource, and not any visible
    /// allocation, mutation or dereference.
    Free,
}

impl Effect for MemoryEffect {}

pub trait MemoryEffectOpInterface = EffectOpInterface<MemoryEffect>;
