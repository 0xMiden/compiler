mod block;
mod builder;
mod callable;
pub mod cfg;
mod component;
mod context;
mod dialect;
pub mod dominance;
pub mod effects;
pub mod entity;
mod ident;
mod immediates;
pub mod loops;
mod op;
mod operands;
mod operation;
pub mod parse;
pub mod print;
mod region;
mod successor;
pub(crate) mod symbols;
pub mod traits;
mod types;
mod usable;
mod value;
pub mod verifier;
mod visit;

pub use midenc_hir_symbol as interner;
pub use midenc_session::diagnostics::{Report, SourceSpan, Span, Spanned};

pub use self::{
    block::{
        Block, BlockCursor, BlockCursorMut, BlockId, BlockList, BlockOperand, BlockOperandRef,
        BlockRef, PostOrderBlockIter, PreOrderBlockIter,
    },
    builder::{Builder, BuilderExt, InsertionGuard, Listener, ListenerType, OpBuilder},
    callable::*,
    context::Context,
    dialect::{
        Dialect, DialectAttributeRegistrationInfo, DialectInfo, DialectOpRegistrationInfo,
        DialectRegistration, DialectRegistrationHook, DialectRegistrationHookInfo,
        DialectRegistrationInfo,
    },
    entity::{
        Entity, EntityGroup, EntityId, EntityList, EntityListCursor, EntityListCursorMut,
        EntityListItem, EntityListIter, EntityMap, EntityMapCursor, EntityMapCursorMut,
        EntityMapItem, EntityMapIter, EntityMut, EntityParent, EntityProjection,
        EntityProjectionMut, EntityRange, EntityRangeMut, EntityRef, EntityStorage, EntityWithId,
        EntityWithKey, EntityWithParent, MaybeDefaultEntityListIter, MaybeDefaultEntityMapIter,
        RawEntityRef, StorableEntity, UnsafeEntityRef, UnsafeIntrusiveEntityRef,
        UnsafeIntrusiveMapEntityRef,
    },
    ident::{FunctionIdent, Ident, IdentAttr},
    immediates::{Felt, FieldElement, Immediate, ImmediateAttr, StarkField},
    op::{BuildableOp, Op, OpExt, OpRegistration},
    operands::{
        OpOperand, OpOperandImpl, OpOperandList, OpOperandRange, OpOperandRangeMut,
        OpOperandStorage,
    },
    operation::{
        AttrInfo, GenericOperationBuilder, OpCursor, OpCursorMut, OpList, Operation,
        OperationBuilder, OperationName, OperationRef, OperationState, PendingSuccessorInfo,
        equivalence,
    },
    parse::{OpAsmParser, OpParser, ParseResult},
    print::{AttrPrinter, OpPrinter, OpPrintingFlags},
    region::{
        InvocationBounds, LoopLikeOpInterface, Region, RegionBranchOpInterface, RegionBranchPoint,
        RegionBranchTerminatorOpInterface, RegionCursor, RegionCursorMut, RegionKind,
        RegionKindInterface, RegionList, RegionRef, RegionSuccessor, RegionSuccessorInfo,
        RegionSuccessorIter, RegionTransformFailed,
    },
    successor::{
        KeyedSuccessor, KeyedSuccessorRange, KeyedSuccessorRangeMut, OpSuccessor, OpSuccessorMut,
        OpSuccessorRange, OpSuccessorRangeMut, OpSuccessorStorage, SuccessorInfo, SuccessorOperand,
        SuccessorOperandRange, SuccessorOperandRangeMut, SuccessorOperands, SuccessorWithKey,
        SuccessorWithKeyMut,
    },
    symbols::*,
    traits::{FoldResult, OpFoldResult},
    types::*,
    usable::Usable,
    value::{
        AsValueRange, BlockArgument, BlockArgumentRange, BlockArgumentRangeMut, BlockArgumentRef,
        OpResult, OpResultRange, OpResultRangeMut, OpResultRef, OpResultStorage, StackOperand,
        Value, ValueId, ValueOrAlias, ValueRange, ValueRef,
    },
    verifier::{OpVerifier, Verify},
    visit::{
        OpVisitor, OperationVisitor, RawWalk, ReverseBlock, Searcher, SymbolVisitor, Visitor, Walk,
        WalkMut, WalkOrder, WalkResult, WalkStage,
    },
};
