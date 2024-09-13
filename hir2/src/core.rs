mod attribute;
mod block;
mod component;
mod context;
mod entity;
mod function;
mod interface;
mod module;
mod op;
mod operation;
mod region;
mod symbol_table;
mod traits;
mod types;
mod usable;
mod value;

pub use self::{
    block::{Block, BlockCursor, BlockCursorMut, BlockList, BlockOperand},
    entity::{
        Entity, EntityCursor, EntityCursorMut, EntityHandle, EntityId, EntityIter, EntityList,
        EntityMut, EntityRef, TrackedEntityHandle,
    },
    function::{
        AbiParam, ArgumentExtension, ArgumentPurpose, CallConv, Function, FunctionIdent, Signature,
    },
    module::Module,
    region::{Region, RegionCursor, RegionCursorMut, RegionList},
    symbol_table::{Symbol, SymbolTable},
    types::*,
    usable::Usable,
    value::{BlockArgument, OpOperand, OpResult, Value, ValueKind},
};
