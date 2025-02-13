mod block;
mod builder;
mod callable;
pub mod cfg;
mod component;
mod context;
mod dialect;
pub mod dominance;
pub(crate) mod entity;
mod ident;
mod immediates;
mod insert;
mod interface;
pub mod loops;
mod op;
mod operands;
mod operation;
mod print;
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
    dialect::{Dialect, DialectInfo, DialectRegistration, DialectRegistrationHook},
    entity::{
        Entity, EntityCursor, EntityCursorMut, EntityGroup, EntityId, EntityIter, EntityList,
        EntityMut, EntityProjection, EntityProjectionMut, EntityRange, EntityRangeMut, EntityRef,
        EntityStorage, EntityWithId, EntityWithParent, MaybeDefaultEntityIter, RawEntityRef,
        StorableEntity, UnsafeEntityRef, UnsafeIntrusiveEntityRef,
    },
    ident::{FunctionIdent, Ident},
    immediates::{Felt, FieldElement, Immediate, StarkField},
    insert::Insert,
    op::{BuildableOp, Op, OpExt, OpRegistration},
    operands::{
        OpOperand, OpOperandImpl, OpOperandList, OpOperandRange, OpOperandRangeMut,
        OpOperandStorage,
    },
    operation::{
        equivalence, OpCursor, OpCursorMut, OpList, Operation, OperationBuilder, OperationName,
        OperationRef,
    },
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
        BlockArgument, BlockArgumentRange, BlockArgumentRangeMut, BlockArgumentRef, OpResult,
        OpResultRange, OpResultRangeMut, OpResultRef, OpResultStorage, Value, ValueId, ValueRange,
        ValueRef,
    },
    verifier::{OpVerifier, Verify},
    visit::{
        OpVisitor, OperationVisitor, RawWalk, Searcher, SymbolVisitor, Visitor, Walk, WalkMut,
        WalkOrder, WalkResult, WalkStage,
    },
};
