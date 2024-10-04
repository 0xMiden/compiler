#![feature(allocator_api)]
#![feature(alloc_layout_extra)]
#![feature(coerce_unsized)]
#![feature(unsize)]
#![feature(ptr_metadata)]
#![feature(layout_for_ptr)]
#![feature(slice_ptr_get)]
#![feature(specialization)]
#![feature(rustc_attrs)]
#![feature(debug_closure_helpers)]
#![feature(trait_alias)]
#![feature(is_none_or)]
#![allow(incomplete_features)]
#![allow(internal_features)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub use compact_str::{
    CompactString as SmallStr, CompactStringExt as SmallStrExt, ToCompactString as ToSmallStr,
};

mod any;
mod attributes;
pub mod demangle;
pub mod derive;
pub mod dialects;
pub mod formatter;
mod ir;

pub use self::{attributes::*, ir::*};

// TODO(pauls): The following is a rough list of what needs to be implemented for the IR
// refactoring to be complete and usable in place of the old IR (some are optional):
//
// * constants and constant-like ops
// * global variables and global ops
// * Builders (i.e. component builder, interface builder, module builder, function builder, last is most important)
//   NOTE: The underlying builder infra is basically done, so layering on the high-level builders is pretty simple
// * canonicalization (optional)
// * visitors (partially complete, need CFG and DFG walkers as well though, largely variations on the existing infra)
// * pattern matching/rewrites (needed for legalization/conversion)
// * dataflow analysis framework (required to replace old analyses)
// * linking/global symbol resolution (required to replace old linker, partially implemented via symbols/symbol tables already)
// * legalization/dialect conversion (required to convert between unstructured and structured control flow dialects at minimum)
// * lowering
