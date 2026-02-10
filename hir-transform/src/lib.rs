#![no_std]
#![feature(new_range_api)]
#![deny(warnings)]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod canonicalization;
mod cfg_to_scf;
//mod dce;
mod cse;
//mod inliner;
mod sccp;
mod sink;
mod spill;

//pub use self::dce::{DeadSymbolElmination, DeadValueElimination};
//pub use self::inliner::Inliner;
pub use self::{
    canonicalization::Canonicalizer,
    cfg_to_scf::{CFGToSCFInterface, transform_cfg_to_scf},
    cse::CommonSubexpressionElimination,
    sccp::SparseConditionalConstantPropagation,
    sink::{ControlFlowSink, SinkOperandDefs},
    spill::{ReloadLike, SpillLike, TransformSpillsInterface, transform_spills},
};
