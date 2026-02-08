#![no_std]
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
#![feature(try_trait_v2)]
#![feature(try_trait_v2_residual)]
#![feature(tuple_trait)]
#![feature(fn_traits)]
#![feature(unboxed_closures)]
#![feature(box_into_inner)]
#![feature(exact_size_is_empty)]
#![feature(generic_const_exprs)]
#![feature(clone_to_uninit)]
#![feature(new_range_api)]
// The following are used in impls of custom collection types based on SmallVec
#![feature(std_internals)] // for ByRefSized
#![feature(extend_one)]
#![feature(extend_one_unchecked)]
#![feature(iter_advance_by)]
#![feature(iter_next_chunk)]
#![feature(iter_collect_into)]
#![feature(trusted_len)]
#![feature(never_type)]
#![feature(cast_maybe_uninit)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_uninit_array_transpose)]
#![feature(array_into_iter_constructors)]
#![feature(slice_range)]
#![feature(slice_swap_unchecked)]
#![feature(hasher_prefixfree_extras)]
// Some of the above features require us to disable these warnings
#![allow(incomplete_features)]
#![allow(internal_features)]
#![deny(warnings)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

extern crate self as midenc_hir;

pub use compact_str::{
    CompactString as SmallStr, CompactStringExt as SmallStrExt, ToCompactString as ToSmallStr,
};
pub use hashbrown;
pub use smallvec::{SmallVec, ToSmallVec, smallvec};

pub type FxHashMap<K, V> = hashbrown::HashMap<K, V, rustc_hash::FxBuildHasher>;
pub type FxHashSet<K> = hashbrown::HashSet<K, rustc_hash::FxBuildHasher>;
pub use rustc_hash::{FxBuildHasher, FxHasher};

pub mod adt;
mod any;
mod attributes;
pub mod constants;
pub mod demangle;
pub mod derive;
pub mod dialects;
mod direction;
mod eq;
mod folder;
pub mod formatter;
mod hash;
mod ir;
pub mod itertools;
pub mod matchers;
pub mod pass;
pub mod patterns;
mod program_point;
pub mod version;

pub use midenc_session::diagnostics;

pub use self::{
    attributes::{
        ArrayAttr, Attribute, AttributeSet, AttributeValue, DictAttr, Overflow, SetAttr,
        Visibility, markers::*,
    },
    direction::{Backward, Direction, Forward},
    eq::DynPartialEq,
    folder::OperationFolder,
    hash::{DynHash, DynHasher},
    ir::*,
    itertools::IteratorExt,
    patterns::{Rewriter, RewriterExt},
    program_point::{Position, ProgramPoint},
};

/// Represents the target of a `log::trace!`/`log::debug!`/etc trace message.
#[derive(Clone)]
pub struct TraceTarget {
    category: interner::Symbol,
    topic: Option<interner::Symbol>,
    relevant_symbol: Option<interner::Symbol>,
    _cached: core::cell::Cell<Option<interner::Symbol>>,
}

impl Default for TraceTarget {
    fn default() -> Self {
        Self {
            category: interner::symbols::Empty,
            topic: None,
            relevant_symbol: None,
            _cached: core::cell::Cell::new(None),
        }
    }
}

impl TraceTarget {
    pub fn category(category: impl Into<interner::Symbol>) -> Self {
        Self {
            category: category.into(),
            ..Default::default()
        }
    }

    pub fn with_topic(mut self, topic: impl Into<interner::Symbol>) -> Self {
        self.topic = Some(topic.into());
        self._cached.set(None);
        self
    }

    pub fn with_relevant_symbol(mut self, symbol: impl Into<interner::Symbol>) -> Self {
        self.relevant_symbol = Some(symbol.into());
        self._cached.set(None);
        self
    }

    pub fn relevant_symbol(&self) -> Option<&'static str> {
        self.relevant_symbol.map(|sym| sym.as_str())
    }
}

impl core::ops::Deref for TraceTarget {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl AsRef<str> for TraceTarget {
    fn as_ref(&self) -> &str {
        match self._cached.get() {
            Some(cached) => cached.as_str(),
            None => {
                let cached = interner::Symbol::intern(self);
                self._cached.set(Some(cached));
                cached.as_str()
            }
        }
    }
}

impl core::fmt::Debug for TraceTarget {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self {
                category,
                topic: None,
                relevant_symbol: None,
                _cached: _,
            } => f.write_str(category.as_str()),
            Self {
                category,
                topic: None,
                relevant_symbol: Some(sym),
                _cached: _,
            } => write!(f, "{category}:{sym}"),
            Self {
                category,
                topic: Some(topic),
                relevant_symbol: None,
                _cached: _,
            } => write!(f, "{category}:{topic}"),
            Self {
                category,
                topic: Some(topic),
                relevant_symbol: Some(sym),
                _cached: _,
            } => write!(f, "{category}:{topic}:{sym}"),
        }
    }
}

impl core::fmt::Display for TraceTarget {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(cached) = self._cached.get() {
            return f.write_str(cached.as_str());
        }

        match self {
            Self {
                category,
                topic: None,
                relevant_symbol: _,
                _cached: _,
            } => f.write_str(category.as_str()),
            Self {
                category,
                topic: Some(topic),
                relevant_symbol: _,
                _cached: _,
            } => write!(f, "{category}:{topic}"),
        }
    }
}
