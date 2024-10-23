#![deny(warnings)]
// TODO: Stabilized in 1.76, then de-stablized before release due to
// a soundness bug when interacting with #![feature(arbitrary_self_types)]
// so this got punted to a later release once they come up with a solution.
//
// Required for pass infrastructure, can be removed when it gets stabilized
// in an upcoming release, see https://github.com/rust-lang/rust/issues/65991
// for details
#![feature(trait_upcasting)]
pub mod parser;

#[cfg(feature = "std")]
extern crate std;

#[macro_use]
extern crate alloc;

#[macro_use]
extern crate lalrpop_util;

pub use intrusive_collections::UnsafeRef;
pub use miden_core::{FieldElement, StarkField};
pub use midenc_hir_macros::*;
pub use midenc_hir_symbol::{symbols, Symbol};
pub use midenc_hir_type::{
    self as types, AddressSpace, Alignable, FunctionType, StructType, Type, TypeRepr,
};
pub use midenc_session::diagnostics::{self, SourceSpan};

pub type FxHashMap<K, V> = hashbrown::HashMap<K, V, rustc_hash::FxBuildHasher>;
pub type FxHashSet<K> = hashbrown::HashSet<K, rustc_hash::FxBuildHasher>;
pub use rustc_hash::{FxBuildHasher, FxHasher};

/// Represents a field element in Miden
pub type Felt = miden_core::Felt;

/// Represents an offset from the base of linear memory in Miden
pub type Offset = u32;

#[macro_export]
macro_rules! assert_matches {
    ($left:expr, $(|)? $( $pattern:pat_param )|+ $( if $guard: expr )? $(,)?) => {
        match $left {
            $( $pattern )|+ $( if $guard )? => {}
            ref left_val => {
                panic!(r#"
assertion failed: `(left matches right)`
    left: `{:?}`,
    right: `{}`"#, left_val, stringify!($($pattern)|+ $(if $guard)?));
            }
        }
    };

    ($left:expr, $(|)? $( $pattern:pat_param )|+ $( if $guard: expr )?, $msg:literal $(,)?) => {
        match $left {
            $( $pattern )|+ $( if $guard )? => {}
            ref left_val => {
                panic!(concat!(r#"
assertion failed: `(left matches right)`
    left: `{:?}`,
    right: `{}`
"#, $msg), left_val, stringify!($($pattern)|+ $(if $guard)?));
            }
        }
    };

    ($left:expr, $(|)? $( $pattern:pat_param )|+ $( if $guard: expr )?, $msg:literal, $($arg:tt)+) => {
        match $left {
            $( $pattern )|+ $( if $guard )? => {}
            ref left_val => {
                panic!(concat!(r#"
assertion failed: `(left matches right)`
    left: `{:?}`,
    right: `{}`
"#, $msg), left_val, stringify!($($pattern)|+ $(if $guard)?), $($arg)+);
            }
        }
    }
}

#[macro_export]
macro_rules! diagnostic {
    ($diagnostics:ident, $severity:expr, $msg:literal) => {{
        $diagnostics.diagnostic($severity).with_message($msg).emit();
    }};

    ($diagnostics:ident, $severity:expr, $msg:literal, $span:expr, $label:expr) => {{
        let span = $span;
        $diagnostics
            .diagnostic($severity)
            .with_message($msg)
            .with_primary_label($span, $label)
            .emit();
    }};

    ($diagnostics:ident, $severity:expr, $msg:literal, $span:expr, $label:expr, $note:expr) => {{
        let span = $span;
        $diagnostics
            .diagnostic($severity)
            .with_message($msg)
            .with_primary_label(span, $label)
            .with_note($note)
            .emit();
    }};

    ($diagnostics:ident, $severity:expr, $msg:literal, $span:expr, $label:expr, $span2:expr, $label2:expr) => {{
        let span = $span;
        let span2 = $span2;
        $diagnostics
            .diagnostic($severity)
            .with_message($msg)
            .with_primary_label(span, $label)
            .with_secondary_label(span2, $label2)
            .emit();
    }};

    ($diagnostics:ident, $severity:expr, $msg:literal, $span:expr, $label:expr, $span2:expr, $label2:expr, $note:expr) => {{
        let span = $span;
        let span2 = $span2;
        $diagnostics
            .diagnostic($severity)
            .with_message($msg)
            .with_primary_label(span, $label)
            .with_secondary_label(span2, $label2)
            .with_help($note)
            .emit();
    }};
}

pub mod adt;
mod asm;
mod attribute;
mod block;
mod builder;
mod component;
mod constants;
mod dataflow;
mod display;
pub mod formatter;
mod function;
mod globals;
mod ident;
mod immediates;
mod insert;
mod instruction;
mod layout;
mod locals;
mod module;
pub mod pass;
mod program;
mod segments;
pub mod testing;
#[cfg(test)]
mod tests;
mod value;

use core::fmt;

// Re-export cranelift_entity so that users don't have to hunt for the same version
pub use cranelift_entity;

pub use self::{
    asm::*,
    attribute::{attributes, Attribute, AttributeSet, AttributeValue},
    block::{Block, BlockData},
    builder::{DefaultInstBuilder, FunctionBuilder, InstBuilder, InstBuilderBase, ReplaceBuilder},
    component::*,
    constants::{Constant, ConstantData, ConstantPool, IntoBytes},
    dataflow::DataFlowGraph,
    display::{Decorator, DisplayValues},
    function::*,
    globals::*,
    ident::{demangle, FunctionIdent, Ident},
    immediates::Immediate,
    insert::{Insert, InsertionPoint},
    instruction::*,
    layout::{ArenaMap, LayoutAdapter, LayoutNode, OrderedArenaMap},
    locals::{Local, LocalId},
    module::*,
    pass::{
        AnalysisKey, ConversionPassRegistration, ModuleRewritePassAdapter, PassInfo,
        RewritePassRegistration,
    },
    program::{Linker, Program, ProgramAnalysisKey, ProgramBuilder},
    segments::{DataSegment, DataSegmentAdapter, DataSegmentError, DataSegmentTable},
    value::{Value, ValueData, ValueList, ValueListPool},
};

/// A `ProgramPoint` represents a position in a function where the live range of an SSA value can
/// begin or end. It can be either:
///
/// 1. An instruction or
/// 2. A block header.
///
/// This corresponds more or less to the lines in the textual form of the IR.
#[derive(PartialEq, Eq, Clone, Copy, Hash)]
pub enum ProgramPoint {
    /// An instruction in the function.
    Inst(Inst),
    /// A block header.
    Block(Block),
}
impl ProgramPoint {
    /// Get the instruction we know is inside.
    pub fn unwrap_inst(self) -> Inst {
        match self {
            Self::Inst(x) => x,
            Self::Block(x) => panic!("expected inst: {}", x),
        }
    }
}
impl From<Inst> for ProgramPoint {
    fn from(inst: Inst) -> Self {
        Self::Inst(inst)
    }
}
impl From<Block> for ProgramPoint {
    fn from(block: Block) -> Self {
        Self::Block(block)
    }
}
impl fmt::Display for ProgramPoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Inst(x) => write!(f, "{}", x),
            Self::Block(x) => write!(f, "{}", x),
        }
    }
}
impl fmt::Debug for ProgramPoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ProgramPoint({})", self)
    }
}
