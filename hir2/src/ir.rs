mod attribute;
mod block;
mod component;
mod context;
mod dialect;
mod entity;
mod function;
mod ident;
mod immediates;
mod insert;
mod interface;
mod module;
mod op;
mod operands;
mod operation;
mod region;
mod successor;
pub(crate) mod symbol_table;
pub mod traits;
mod types;
mod usable;
mod value;
pub(crate) mod verifier;
mod visit;

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
    insert::{Insert, InsertionPoint, ProgramPoint},
    module::Module,
    op::{Op, OpExt},
    operands::{
        OpOperand, OpOperandImpl, OpOperandList, OpOperandRange, OpOperandRangeMut,
        OpOperandStorage,
    },
    operation::{
        OpCursor, OpCursorMut, OpList, Operation, OperationBuilder, OperationName, OperationRef,
    },
    region::{Region, RegionCursor, RegionCursorMut, RegionList, RegionRef},
    successor::OpSuccessor,
    symbol_table::{
        Symbol, SymbolName, SymbolNameAttr, SymbolNameComponent, SymbolRef, SymbolTable, SymbolUse,
        SymbolUseCursor, SymbolUseCursorMut, SymbolUseIter, SymbolUseList, SymbolUseRef,
    },
    types::*,
    usable::Usable,
    value::{BlockArgument, BlockArgumentRef, OpResult, OpResultRef, Value, ValueId, ValueRef},
    verifier::{OpVerifier, Verify},
    visit::{OpVisitor, OperationVisitor, Searcher, SymbolVisitor, Visitor},
};
