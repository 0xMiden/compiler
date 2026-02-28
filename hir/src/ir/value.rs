mod aliasing;
mod range;
mod stack;

use core::{any::Any, fmt};

pub use self::{
    aliasing::ValueOrAlias,
    range::{AsValueRange, ValueRange},
    stack::StackOperand,
};
use super::*;
use crate::{DynHash, DynPartialEq, PartialEqable, any::AsAny, interner};

/// A unique identifier for a [Value] in the IR
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ValueId(u32);

impl ValueId {
    /// The shift used to offset the actual unique value id into untagged bits
    const ID_SHIFT: u32 = 6;
    /// The maximum value of any raw u32 value used as a [ValueId]
    const MAX_VALUE_ID: u32 = (u8::MAX as u32) << 24;
    /// 6 bits are reserved for op result indices, used when OP_RESULT_TAG is set
    const OP_RESULT_INDEX_MASK: u32 = (u8::MAX as u32) >> 2;
    /// 1 bit is reserved for marking op result value ids
    const OP_RESULT_TAG: u32 = 1u32 << 30;
    /// 1 bit is reserved for marking user-defined symbols
    const USER_DEFINED_TAG: u32 = 1u32 << 31;

    /// Create a [ValueId] from a [Symbol](interner::Symbol) representing a user-defined name.
    ///
    /// This is used when parsing IR, so that we can preserve the user-provided names.
    pub const fn from_symbol(sym: interner::Symbol) -> Self {
        // Symbol guarantees that 8 bits of its 32-bit repr are reserved for uses like this
        let sym = sym.as_u32();
        debug_assert!(
            sym < Self::MAX_VALUE_ID,
            "cannot convert symbol id to value id: bits set in reserved range"
        );
        Self((sym << Self::ID_SHIFT) | Self::USER_DEFINED_TAG)
    }

    /// Returns true if this [ValueId] was user-defined
    pub const fn is_user_defined(self) -> bool {
        self.0 & Self::USER_DEFINED_TAG == Self::USER_DEFINED_TAG
    }

    /// Returns true if this [ValueId] represents an operation result
    pub const fn is_op_result(self) -> bool {
        self.0 & Self::OP_RESULT_TAG == Self::OP_RESULT_TAG
    }

    /// Returns the index of the operation result that this value id corresponds to.
    ///
    /// Returns `None` if this [ValueId] does not represent a compressed operation result, e.g.
    /// values of the form `%result:2`, where `%result` is the name bound to some operation's
    /// results, of which there are 2.
    pub const fn result_index(self) -> Option<u8> {
        if self.is_op_result() {
            Some((self.0 & Self::OP_RESULT_INDEX_MASK) as u8)
        } else {
            None
        }
    }

    /// Convert this [ValueId] into one which represents the `index`th result of some operation.
    pub const fn with_result_index(self, index: u8) -> Self {
        assert!(
            index as u32 <= Self::OP_RESULT_INDEX_MASK,
            "invalid op result index: must be less than 64",
        );
        Self((self.0 & !Self::OP_RESULT_INDEX_MASK) | index as u32)
    }

    /// Strip operation result metadata from this [ValueId]
    ///
    /// This is used during parsing to look up definitions of a value whose name is shared across
    /// multiple operation results, without incorporating the result index into the hash.
    pub(super) const fn without_result_index(self) -> Self {
        Self(self.0 & !(Self::OP_RESULT_INDEX_MASK | Self::OP_RESULT_TAG))
    }

    pub const fn from_u32(id: u32) -> Self {
        assert!(id < Self::MAX_VALUE_ID, "invalid value id: bits set in reserved range");
        Self(id << Self::ID_SHIFT)
    }

    pub const fn as_u32(&self) -> u32 {
        (self.0 >> Self::ID_SHIFT) & !(Self::USER_DEFINED_TAG | Self::OP_RESULT_TAG)
    }

    pub const fn as_symbol_id(self) -> Option<interner::Symbol> {
        if self.is_user_defined() {
            Some(unsafe { core::mem::transmute::<u32, interner::Symbol>(self.as_u32()) })
        } else {
            None
        }
    }
}

impl EntityId for ValueId {
    #[inline(always)]
    fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

impl fmt::Debug for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            f.debug_struct("ValueId")
                .field("is_user_defined", &self.is_user_defined())
                .field("is_op_result", &self.is_op_result())
                .field("op_result_index", &self.result_index())
                .field("id", &self.as_u32())
                .finish()
        } else {
            fmt::Display::fmt(self, f)
        }
    }
}

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(sym) = self.as_symbol_id() {
            write!(f, "%{sym}")
        } else if let Some(index) = self.result_index() {
            write!(f, "%{}#{index}", self.as_u32())
        } else {
            write!(f, "%{}", self.as_u32())
        }
    }
}

/// Represents an SSA value in the IR.
///
/// The data underlying a [Value] represents a _definition_, and thus implements [Usable]. The users
/// of a [Value] are operands (see [OpOperandImpl]). Operands are associated with an operation. Thus
/// the graph formed of the edges between values and operations via operands forms the data-flow
/// graph of the program.
pub trait Value:
    AsAny
    + EntityWithId<Id = ValueId>
    + Spanned
    + Usable<Use = OpOperandImpl>
    + fmt::Debug
    + fmt::Display
    + PartialEqable
    + DynPartialEq
    + DynHash
{
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Set the source location of this value
    fn set_span(&mut self, span: SourceSpan);
    /// Get the type of this value
    fn ty(&self) -> &Type;
    /// Set the type of this value
    fn set_type(&mut self, ty: Type);
    /// Get the defining operation for this value, _if_ defined by an operation.
    ///
    /// Returns `None` if this value is defined by other means than an operation result.
    fn get_defining_op(&self) -> Option<OperationRef>;
    /// Get the region which contains the definition of this value
    fn parent_region(&self) -> Option<RegionRef> {
        self.parent_block().and_then(|block| block.parent())
    }
    /// Get the block which contains the definition of this value
    fn parent_block(&self) -> Option<BlockRef>;
    /// Returns true if this value is used outside of the given block
    fn is_used_outside_of_block(&self, block: &BlockRef) -> bool {
        self.iter_uses()
            .any(|user| user.owner.parent().is_some_and(|blk| !BlockRef::ptr_eq(&blk, block)))
    }
    /// Replace all uses of `self` with `replacement`
    fn replace_all_uses_with(&mut self, mut replacement: ValueRef) {
        let mut cursor = self.uses_mut().front_mut();
        while let Some(mut user) = cursor.as_pointer() {
            // Rewrite use of `self` with `replacement`
            {
                let mut user = user.borrow_mut();
                user.value = Some(replacement);
            }
            // Remove `user` from the use list of `self`
            cursor.remove();
            // Add `user` to the use list of `replacement`
            replacement.borrow_mut().insert_use(user);
        }
    }
    /// Replace all uses of `self` with `replacement` unless the user is in `exceptions`
    fn replace_all_uses_except(&mut self, mut replacement: ValueRef, exceptions: &[OperationRef]) {
        let mut cursor = self.uses_mut().front_mut();
        while let Some(mut user) = cursor.as_pointer() {
            // Rewrite use of `self` with `replacement` if user not in `exceptions`
            {
                let mut user = user.borrow_mut();
                if exceptions.contains(&user.owner) {
                    cursor.move_next();
                    continue;
                }
                user.value = Some(replacement);
            }
            // Remove `user` from the use list of `self`
            cursor.remove();
            // Add `user` to the use list of `replacement`
            replacement.borrow_mut().insert_use(user);
        }
    }
}

impl dyn Value {
    #[inline]
    pub fn is<T: Value>(&self) -> bool {
        Value::as_any(self).is::<T>()
    }

    #[inline]
    pub fn downcast_ref<T: Value>(&self) -> Option<&T> {
        Value::as_any(self).downcast_ref::<T>()
    }

    #[inline]
    pub fn downcast_mut<T: Value>(&mut self) -> Option<&mut T> {
        Value::as_any_mut(self).downcast_mut::<T>()
    }

    /// Replace all uses of `self` with `replacement` if `should_replace` returns true
    pub fn replace_uses_with_if<F>(&mut self, mut replacement: ValueRef, should_replace: F)
    where
        F: Fn(&OpOperandImpl) -> bool,
    {
        let mut cursor = self.uses_mut().front_mut();
        while let Some(mut user) = cursor.as_pointer() {
            // Rewrite use of `self` with `replacement` if `should_replace` returns true
            {
                let mut user = user.borrow_mut();
                if !should_replace(&user) {
                    cursor.move_next();
                    continue;
                }
                user.value = Some(replacement);
            }
            // Remove `user` from the use list of `self`
            cursor.remove();
            // Add `user` to the use list of `replacement`
            replacement.borrow_mut().insert_use(user);
        }
    }
}

/// Generates the boilerplate for a concrete [Value] type.
macro_rules! value_impl {
    (
        $(#[$outer:meta])*
        $vis:vis struct $ValueKind:ident {
            $(#[doc $($owner_doc_args:tt)*])*
            owner: $OwnerTy:ty,
            $(#[doc $($index_doc_args:tt)*])*
            index: u8,
            $(
                $(#[$inner:ident $($args:tt)*])*
                $Field:ident: $FieldTy:ty,
            )*
        }

        fn get_defining_op(&$GetDefiningOpSelf:ident) -> Option<OperationRef> $GetDefiningOp:block

        fn parent_block(&$ParentBlockSelf:ident) -> Option<BlockRef> $ParentBlock:block

        $($t:tt)*
    ) => {
        $(#[$outer])*
        #[derive(Spanned)]
        $vis struct $ValueKind {
            id: ValueId,
            #[span]
            span: SourceSpan,
            ty: Type,
            uses: OpOperandList,
            owner: $OwnerTy,
            index: u8,
            $(
                $(#[$inner $($args)*])*
                $Field: $FieldTy
            ),*
        }


        impl $ValueKind {
            pub fn new(
                span: SourceSpan,
                id: ValueId,
                ty: Type,
                owner: $OwnerTy,
                index: u8,
                $(
                    $Field: $FieldTy
                ),*
            ) -> Self {
                Self {
                    id,
                    ty,
                    span,
                    uses: Default::default(),
                    owner,
                    index,
                    $(
                        $Field
                    ),*
                }
            }

            $(#[doc $($owner_doc_args)*])*
            pub fn owner(&self) -> $OwnerTy {
                self.owner.clone()
            }

            $(#[doc $($index_doc_args)*])*
            pub fn index(&self) -> usize {
                self.index as usize
            }
        }

        impl Value for $ValueKind {
            #[inline(always)]
            fn as_any(&self) -> &dyn Any {
                self
            }
            #[inline(always)]
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
            fn ty(&self) -> &Type {
                &self.ty
            }

            fn set_span(&mut self, span: SourceSpan) {
                self.span = span;
            }

            fn set_type(&mut self, ty: Type) {
                self.ty = ty;
            }

            fn get_defining_op(&$GetDefiningOpSelf) -> Option<OperationRef> $GetDefiningOp

            fn parent_block(&$ParentBlockSelf) -> Option<BlockRef> $ParentBlock
        }

        impl Entity for $ValueKind {}
        impl EntityWithId for $ValueKind {
            type Id = ValueId;

            #[inline(always)]
            fn id(&self) -> Self::Id {
                self.id
            }
        }

        impl EntityParent<OpOperandImpl> for $ValueKind {
            fn offset() -> usize {
                core::mem::offset_of!($ValueKind, uses)
            }
        }

        impl Usable for $ValueKind {
            type Use = OpOperandImpl;

            #[inline(always)]
            fn uses(&self) -> &OpOperandList {
                &self.uses
            }

            #[inline(always)]
            fn uses_mut(&mut self) -> &mut OpOperandList {
                &mut self.uses
            }
        }


        impl Eq for $ValueKind {}

        impl PartialEq for $ValueKind {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }

        impl Ord for $ValueKind {
            fn cmp(&self, other: &Self) -> core::cmp::Ordering {
                self.id.cmp(&other.id)
            }
        }

        impl PartialOrd for $ValueKind {
            fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl core::hash::Hash for $ValueKind {
            fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
                self.id.hash(state);
            }
        }

        impl fmt::Display for $ValueKind {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                use crate::formatter::PrettyPrint;

                self.pretty_print(f)
            }
        }

        impl fmt::Debug for $ValueKind {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let mut builder = f.debug_struct(stringify!($ValueKind));
                builder
                    .field("id", &self.id)
                    .field("ty", &self.ty)
                    .field("index", &self.index)
                    .field("is_used", &(!self.uses.is_empty()));

                $(
                    builder.field(stringify!($Field), &self.$Field);
                )*

                builder.finish_non_exhaustive()
            }
        }

        $($t)*
    }
}

/// A pointer to a [Value]
pub type ValueRef = UnsafeEntityRef<dyn Value>;
/// A pointer to a [BlockArgument]
pub type BlockArgumentRef = UnsafeEntityRef<BlockArgument>;
/// A pointer to a [OpResult]
pub type OpResultRef = UnsafeEntityRef<OpResult>;

value_impl!(
    /// A [BlockArgument] represents the definition of a [Value] by a block parameter
    pub struct BlockArgument {
        /// Get the [Block] to which this [BlockArgument] belongs
        owner: BlockRef,
        /// Get the index of this argument in the argument list of the owning [Block]
        index: u8,
    }

    fn get_defining_op(&self) -> Option<OperationRef> {
        None
    }

    fn parent_block(&self) -> Option<BlockRef> {
        Some(self.owner)
    }
);

impl BlockArgument {
    #[inline]
    pub fn as_value_ref(&self) -> ValueRef {
        self.as_block_argument_ref() as ValueRef
    }

    #[inline]
    pub fn as_block_argument_ref(&self) -> BlockArgumentRef {
        unsafe { BlockArgumentRef::from_raw(self) }
    }
}

impl crate::formatter::PrettyPrint for BlockArgument {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        display(self.id) + const_text(": ") + self.ty.render()
    }
}

impl StorableEntity for BlockArgument {
    #[inline(always)]
    fn index(&self) -> usize {
        self.index as usize
    }

    unsafe fn set_index(&mut self, index: usize) {
        self.index = index.try_into().expect("too many block arguments");
    }
}

pub type BlockArgumentRange<'a> = crate::EntityRange<'a, BlockArgumentRef>;
pub type BlockArgumentRangeMut<'a> = crate::EntityRangeMut<'a, BlockArgumentRef, 1>;

value_impl!(
    /// An [OpResult] represents the definition of a [Value] by the result of an [Operation]
    pub struct OpResult {
        /// Get the [Operation] to which this [OpResult] belongs
        owner: OperationRef,
        /// Get the index of this result in the result list of the owning [Operation]
        index: u8,
    }

    fn get_defining_op(&self) -> Option<OperationRef> {
        Some(self.owner)
    }

    fn parent_block(&self) -> Option<BlockRef> {
        self.owner.parent()
    }
);

impl OpResult {
    #[inline]
    pub fn as_value_ref(&self) -> ValueRef {
        unsafe { ValueRef::from_raw(self as &dyn Value) }
    }

    #[inline]
    pub fn as_op_result_ref(&self) -> OpResultRef {
        unsafe { OpResultRef::from_raw(self) }
    }
}

impl crate::formatter::PrettyPrint for OpResult {
    #[inline]
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        display(self.id)
    }
}

impl StorableEntity for OpResult {
    #[inline(always)]
    fn index(&self) -> usize {
        self.index as usize
    }

    unsafe fn set_index(&mut self, index: usize) {
        self.index = index.try_into().expect("too many op results");
    }

    /// Unlink all users of this result
    ///
    /// The users will still refer to this result, but the use list of this value will be empty
    fn unlink(&mut self) {
        let uses = self.uses_mut();
        uses.clear();
    }
}

pub type OpResultStorage = crate::EntityStorage<OpResultRef, 1>;
pub type OpResultRange<'a> = crate::EntityRange<'a, OpResultRef>;
pub type OpResultRangeMut<'a> = crate::EntityRangeMut<'a, OpResultRef, 1>;
