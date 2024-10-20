use core::{any::Any, fmt};

use super::*;

/// A unique identifier for a [Value] in the IR
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ValueId(u32);
impl ValueId {
    pub const fn from_u32(id: u32) -> Self {
        Self(id)
    }

    pub const fn as_u32(&self) -> u32 {
        self.0
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
        write!(f, "v{}", &self.0)
    }
}
impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", &self.0)
    }
}

/// Represents an SSA value in the IR.
///
/// The data underlying a [Value] represents a _definition_, and thus implements [Usable]. The users
/// of a [Value] are operands (see [OpOperandImpl]). Operands are associated with an operation. Thus
/// the graph formed of the edges between values and operations via operands forms the data-flow
/// graph of the program.
pub trait Value:
    EntityWithId<Id = ValueId> + Spanned + Usable<Use = OpOperandImpl> + fmt::Debug
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
        self.parent_block().and_then(|block| block.borrow().parent())
    }
    /// Get the block which contains the definition of this value
    fn parent_block(&self) -> Option<BlockRef>;
    /// Returns true if this value is used outside of the given block
    fn is_used_outside_of_block(&self, block: &BlockRef) -> bool {
        self.iter_uses().any(|user| {
            user.owner.borrow().parent().is_some_and(|blk| !BlockRef::ptr_eq(&blk, block))
        })
    }
    /// Replace all uses of `self` with `replacement`
    fn replace_all_uses_with(&mut self, mut replacement: ValueRef) {
        let mut cursor = self.uses_mut().front_mut();
        while let Some(mut user) = cursor.as_pointer() {
            // Rewrite use of `self` with `replacement`
            {
                let mut user = user.borrow_mut();
                user.value = replacement.clone();
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
                user.value = replacement.clone();
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
        self.as_any().is::<T>()
    }

    #[inline]
    pub fn downcast_ref<T: Value>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    #[inline]
    pub fn downcast_mut<T: Value>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
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
                user.value = replacement.clone();
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
        Some(self.owner.clone())
    }
);

impl BlockArgument {
    #[inline]
    pub fn as_value_ref(&self) -> ValueRef {
        self.as_block_argument_ref().upcast()
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

value_impl!(
    /// An [OpResult] represents the definition of a [Value] by the result of an [Operation]
    pub struct OpResult {
        /// Get the [Operation] to which this [OpResult] belongs
        owner: OperationRef,
        /// Get the index of this result in the result list of the owning [Operation]
        index: u8,
    }

    fn get_defining_op(&self) -> Option<OperationRef> {
        Some(self.owner.clone())
    }

    fn parent_block(&self) -> Option<BlockRef> {
        self.owner.borrow().parent()
    }
);

impl OpResult {
    #[inline]
    pub fn as_value_ref(&self) -> ValueRef {
        unsafe { ValueRef::from_raw(self as &dyn Value) }
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
