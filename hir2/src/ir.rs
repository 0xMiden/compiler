mod attribute;
mod block;
mod component;
mod context;
mod dialect;
mod entity;
mod function;
mod ident;
mod immediates;
mod interface;
mod module;
mod op;
mod operation;
mod region;
mod symbol_table;
pub mod traits;
mod types;
mod usable;
mod value;
pub(crate) mod verifier;

pub use midenc_hir_symbol as interner;
pub use midenc_session::diagnostics::{Report, SourceSpan, Spanned};

pub use self::{
    attribute::{attributes::*, Attribute, AttributeSet, AttributeValue},
    block::{
        Block, BlockCursor, BlockCursorMut, BlockId, BlockList, BlockOperand, BlockOperandRef,
        BlockRef,
    },
    context::Context,
    dialect::{Dialect, DialectName},
    entity::{
        Entity, EntityCursor, EntityCursorMut, EntityId, EntityIter, EntityList, EntityMut,
        EntityRef, RawEntityRef, UnsafeEntityRef, UnsafeIntrusiveEntityRef,
    },
    function::{AbiParam, ArgumentExtension, ArgumentPurpose, Function, Signature},
    ident::{FunctionIdent, Ident},
    immediates::{Felt, FieldElement, Immediate, StarkField},
    module::Module,
    op::{Op, OpExt},
    operation::{
        OpCursor, OpCursorMut, OpList, OpSuccessor, Operation, OperationBuilder, OperationName,
        OperationRef,
    },
    region::{Region, RegionCursor, RegionCursorMut, RegionList, RegionRef},
    symbol_table::{Symbol, SymbolTable},
    types::*,
    usable::Usable,
    value::{
        BlockArgument, BlockArgumentRef, OpOperand, OpResult, OpResultRef, Value, ValueId, ValueRef,
    },
    verifier::{OpVerifier, Verify},
};
