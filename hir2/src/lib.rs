#![feature(allocator_api)]
#![feature(alloc_layout_extra)]
#![feature(coerce_unsized)]
#![feature(unsize)]
#![feature(ptr_metadata)]
#![feature(ptr_as_uninit)]
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
#![feature(box_into_inner)]
#![feature(const_type_id)]
#![feature(exact_size_is_empty)]
#![feature(generic_const_exprs)]
#![feature(new_uninit)]
#![feature(clone_to_uninit)]
// The following are used in impls of custom collection types based on SmallVec
#![feature(iter_repeat_n)]
#![feature(std_internals)] // for ByRefSized
#![feature(extend_one)]
#![feature(extend_one_unchecked)]
#![feature(iter_advance_by)]
#![feature(iter_next_chunk)]
#![feature(iter_collect_into)]
#![feature(trusted_len)]
#![feature(never_type)]
#![feature(maybe_uninit_slice)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_uninit_array_transpose)]
#![feature(array_into_iter_constructors)]
#![feature(slice_range)]
#![feature(slice_swap_unchecked)]
#![feature(hasher_prefixfree_extras)]
// Some of the above features require us to disable these warnings
#![allow(incomplete_features)]
#![allow(internal_features)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

extern crate self as midenc_hir2;

pub use compact_str::{
    CompactString as SmallStr, CompactStringExt as SmallStrExt, ToCompactString as ToSmallStr,
};

pub type FxHashMap<K, V> = hashbrown::HashMap<K, V, rustc_hash::FxBuildHasher>;
pub type FxHashSet<K> = hashbrown::HashSet<K, rustc_hash::FxBuildHasher>;
pub use rustc_hash::{FxBuildHasher, FxHasher};

pub mod adt;
mod any;
mod attributes;
pub mod constants;
pub mod dataflow;
pub mod demangle;
pub mod derive;
pub mod dialects;
mod eq;
mod folder;
pub mod formatter;
mod hash;
mod ir;
pub mod itertools;
pub mod matchers;
pub mod pass;
mod patterns;
pub mod transforms;
pub mod version;

pub use self::{
    attributes::{
        markers::*, Attribute, AttributeSet, AttributeValue, CallConv, DictAttr, Overflow, SetAttr,
        Visibility,
    },
    eq::DynPartialEq,
    folder::OperationFolder,
    hash::{DynHash, DynHasher},
    ir::*,
    itertools::IteratorExt,
    patterns::*,
};

// TODO(pauls): The following is a rough list of what needs to be implemented for the IR
// refactoring to be complete and usable in place of the old IR (some are optional):
//
// * canonicalization (optional)
// * pattern matching/rewrites (needed for legalization/conversion, mostly complete, see below)
// * linking/global symbol resolution (required to replace old linker, partially implemented via symbols/symbol tables already)
// * legalization/dialect conversion (required to convert between unstructured and structured control flow dialects at minimum)
// * lowering
