mod attribute;
mod block;
mod builder;
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
mod print;
mod region;
mod successor;
pub(crate) mod symbol_table;
pub mod traits;
mod types;
mod usable;
mod value;
pub mod verifier;
mod visit;

pub use midenc_hir_symbol as interner;
pub use midenc_session::diagnostics::{Report, SourceSpan, Span, Spanned};

pub use self::{
    attribute::{attributes::*, Attribute, AttributeSet, AttributeValue, DictAttr, SetAttr},
    block::{
        Block, BlockCursor, BlockCursorMut, BlockId, BlockList, BlockOperand, BlockOperandRef,
        BlockRef,
    },
    builder::{Builder, Listener, ListenerType, OpBuilder},
    context::Context,
    dialect::{Dialect, DialectName, DialectRegistration},
    entity::{
        Entity, EntityCursor, EntityCursorMut, EntityGroup, EntityId, EntityIter, EntityList,
        EntityMut, EntityRange, EntityRangeMut, EntityRef, EntityStorage, RawEntityRef,
        StorableEntity, UnsafeEntityRef, UnsafeIntrusiveEntityRef,
    },
    function::{AbiParam, ArgumentExtension, ArgumentPurpose, Function, Signature},
    ident::{FunctionIdent, Ident},
    immediates::{Felt, FieldElement, Immediate, StarkField},
    insert::{Insert, InsertionPoint, ProgramPoint},
    module::Module,
    op::{BuildableOp, Op, OpExt, OpRegistration},
    operands::{
        OpOperand, OpOperandImpl, OpOperandList, OpOperandRange, OpOperandRangeMut,
        OpOperandStorage,
    },
    operation::{
        OpCursor, OpCursorMut, OpList, Operation, OperationBuilder, OperationName, OperationRef,
    },
    print::OpPrinter,
    region::{Region, RegionCursor, RegionCursorMut, RegionList, RegionRef},
    successor::{
        KeyedSuccessor, KeyedSuccessorRange, KeyedSuccessorRangeMut, OpSuccessor, OpSuccessorMut,
        OpSuccessorRange, OpSuccessorRangeMut, OpSuccessorStorage, SuccessorInfo, SuccessorWithKey,
        SuccessorWithKeyMut,
    },
    symbol_table::{
        AsSymbolRef, InvalidSymbolRefError, Symbol, SymbolName, SymbolNameAttr,
        SymbolNameComponent, SymbolNameComponents, SymbolRef, SymbolTable, SymbolUse,
        SymbolUseCursor, SymbolUseCursorMut, SymbolUseIter, SymbolUseList, SymbolUseRef,
        SymbolUsesIter,
    },
    types::*,
    usable::Usable,
    value::{
        BlockArgument, BlockArgumentRef, OpResult, OpResultRange, OpResultRangeMut, OpResultRef,
        OpResultStorage, Value, ValueId, ValueRef,
    },
    verifier::{OpVerifier, Verify},
    visit::{
        OpVisitor, OperationVisitor, Searcher, SymbolVisitor, Visitor, WalkOrder, WalkResult,
        WalkStage, Walkable,
    },
};
