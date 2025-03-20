/// Represents the types of regions that can be represented in the IR
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum RegionKind {
    /// A graph region is one without control-flow semantics, i.e. dataflow between operations is
    /// the only thing that dictates order, and operations can be conceptually executed in parallel
    /// if the runtime supports it.
    ///
    /// As there is no control-flow in these regions, graph regions may only contain a single block.
    Graph,
    /// An SSA region is one where the strict control-flow semantics and properties of SSA (static
    /// single assignment) form must be upheld.
    ///
    /// SSA regions must adhere to:
    ///
    /// * Values can only be defined once
    /// * Definitions must dominate uses
    /// * Ordering of operations in a block corresponds to execution order, i.e. operations earlier
    ///   in a block dominate those later in the block.
    /// * Blocks must end with a terminator.
    #[default]
    SSA,
}
