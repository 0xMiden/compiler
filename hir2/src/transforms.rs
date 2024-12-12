mod canonicalization;
//mod cfg_to_scf;
//mod cse;
//mod dce;
//mod inliner;
mod sccp;
mod spill;

//pub use self::cfg_to_scf::StructuredControlFlowRecovery;
//pub use self::cse::CommonSubexpressionElimination;
//pub use self::dce::{DeadSymbolElmination, DeadValueElimination};
//pub use self::inliner::Inliner;
pub use self::sccp::SparseConditionalConstantPropagation;
pub use self::{canonicalization::Canonicalizer, spill::InsertSpills};
