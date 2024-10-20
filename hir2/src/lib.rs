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
#![feature(trait_upcasting)]
#![feature(is_none_or)]
#![feature(try_trait_v2)]
#![feature(try_trait_v2_residual)]
#![feature(tuple_trait)]
#![feature(fn_traits)]
#![feature(unboxed_closures)]
#![feature(const_type_id)]
#![feature(exact_size_is_empty)]
#![allow(incomplete_features)]
#![allow(internal_features)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

extern crate self as midenc_hir2;

pub use compact_str::{
    CompactString as SmallStr, CompactStringExt as SmallStrExt, ToCompactString as ToSmallStr,
};

mod any;
mod attributes;
pub mod demangle;
pub mod derive;
pub mod dialects;
mod folder;
pub mod formatter;
mod hash;
mod ir;
pub mod matchers;
mod patterns;

pub use self::{
    any::AsAny,
    attributes::{
        markers::*, Attribute, AttributeSet, AttributeValue, CallConv, DictAttr, Overflow, SetAttr,
        Visibility,
    },
    folder::OperationFolder,
    hash::{DynHash, DynHasher},
    ir::*,
    patterns::*,
};

// TODO(pauls): The following is a rough list of what needs to be implemented for the IR
// refactoring to be complete and usable in place of the old IR (some are optional):
//
// * constants and constant-like ops
// * global variables and global ops
// * Need to implement InferTypeOpInterface for all applicable ops
// * Builders (i.e. component builder, interface builder, module builder, function builder, last is most important)
//   NOTE: The underlying builder infra is done, so layering on the high-level builders is pretty simple
// * canonicalization (optional)
// * pattern matching/rewrites (needed for legalization/conversion, mostly complete, see below)
//   - Need to provide implementations of stubbed out rewriter methods
//   - Need to implement the GreedyRewritePatternDriver
//   - Need to implement matchers
// * dataflow analysis framework (required to replace old analyses)
// * linking/global symbol resolution (required to replace old linker, partially implemented via symbols/symbol tables already)
// * legalization/dialect conversion (required to convert between unstructured and structured control flow dialects at minimum)
// * lowering
