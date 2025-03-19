#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Speculatability {
    /// The Operation in question cannot be speculatively executed.  This could be
    /// because it may invoke undefined behavior or have other side effects.
    NotSpeculatable,

    /// The Operation in question can be speculatively executed.  It does not have
    /// any side effects or undefined behavior.
    Speculatable,

    /// The Operation in question can be speculatively executed if all the
    /// operations in all attached regions can also be speculatively executed.
    RecursivelySpeculatable,
}

/// This op interface enables one to inject custom logic to determine whether an Operation can be
/// speculatively executed.
///
/// Ops that implement this interface need to implement the custom logic in the `speculatability`
/// method.
///
/// For instance, the `speculatability` for a specific op may check the attributes or input types to
/// determine whether that specific operation is speculatable.
pub trait ConditionallySpeculatable {
    /// Returns value indicating whether the specific operation in question can be speculatively
    /// executed.
    ///
    /// Please see the [Speculatability] docs to know how to interpret the return value.
    fn speculatability(&self) -> Speculatability;
}

/// This trait marks an op (which must be tagged as implementing the [ConditionallySpeculatable]
/// interface) as being recursively speculatable.
///
/// This means that said op can be speculated only if all the instructions in all the regions
/// attached to the op can be speculated.
pub trait RecursivelySpeculatable: ConditionallySpeculatable {}

/// This trait marks an op (which must be tagged as implementing the [ConditionallySpeculatable]
/// interface) as being always speculatable.
pub trait AlwaysSpeculatable: ConditionallySpeculatable {}
