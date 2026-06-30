use super::*;

/// Debug effects are similar to memory effects in that they reflect how a debugger may observe the
/// effect during execution/debugging.
///
/// Similarly, optimizations must avoid reordering operations around debug effects in the same way
/// they must not reorder around memory effects (i.e. an op with a `write` memory effect on some
/// resource must not be reordered before an op with a `read` debug effect on that same resource).
/// In practice, debug operations may declare both memory effects and debug effects, to ensure that
/// transformations which are unaware of debug effects still do the right thing with respect to
/// those operations - but this should be considered a last resort.
///
/// An operation whose value uses only include debug effects, are ignored when considering the
/// liveness of those values. This allows debug metadata to be recorded in the use-def graph,
/// without interfering with dead-code elimination and other similar optimizations.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DebugEffect {
    /// The following effect indicates that the operation reads from some resource.
    ///
    /// A 'read' effect implies that a debugger may attempt dereferencing of the resource
    Read,
    /// The following effect indicates that the operation writes to some resource.
    ///
    /// A 'write' effect implies that a debugger will modify its internal state with respect to
    /// some resource (e.g. the storage type or location of a value). This effect only describes
    /// mutation of the state, not any visible dereference or read.
    Write,
    /// The following effect indicates that the operation allocates some resource.
    ///
    /// An 'allocate' effect implies only allocation of the resource, and not any visible mutation or
    /// dereference. In the case of a debugger, this might correspond to allocating a new call frame
    /// or start tracking the state of a local variable.
    Allocate,
    /// The following effect indicates that the operation frees some resource that has been
    /// allocated.
    ///
    /// A 'free' effect implies only de-allocation of the resource, and not any visible
    /// allocation, mutation or dereference. In a debugging context, this might correspond to
    /// popping a frame from the call stack, or marking the end of the live range of some local
    /// variable.
    Free,
}

impl PartialEq<DebugEffect> for &DebugEffect {
    #[inline]
    fn eq(&self, other: &DebugEffect) -> bool {
        (**self).eq(other)
    }
}

impl Effect for DebugEffect {}

pub trait DebugEffectOpInterface = EffectOpInterface<DebugEffect>;
