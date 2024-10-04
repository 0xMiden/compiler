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
pub trait Value: Entity<Id = ValueId> + Spanned + Usable<Use = OpOperandImpl> + fmt::Debug {
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
}

/// Generates the boilerplate for a concrete [Value] type.
macro_rules! value_impl {
    (
        $(#[$outer:meta])*
        $vis:vis struct $ValueKind:ident {
            $(
                $(*[$inner:ident $($args:tt)*])*
                $Field:ident: $FieldTy:ty,
            )*
        }

        fn get_defining_op(&$GetDefiningOpSelf:ident) -> Option<OperationRef> $GetDefiningOp:block

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
                $(
                    $Field: $FieldTy
                ),*
            ) -> Self {
                Self {
                    id,
                    ty,
                    span,
                    uses: Default::default(),
                    $(
                        $Field
                    ),*
                }
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
        }

        impl Entity for $ValueKind {
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

        impl fmt::Debug for $ValueKind {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let mut builder = f.debug_struct(stringify!($ValueKind));
                builder
                    .field("id", &self.id)
                    .field("ty", &self.ty)
                    .field("uses", &self.uses);

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
        owner: BlockRef,
        index: u8,
    }

    fn get_defining_op(&self) -> Option<OperationRef> {
        None
    }
);

value_impl!(
    /// An [OpResult] represents the definition of a [Value] by the result of an [Operation]
    pub struct OpResult {
        owner: OperationRef,
        index: u8,
    }

    fn get_defining_op(&self) -> Option<OperationRef> {
        Some(self.owner.clone())
    }
);

impl BlockArgument {
    /// Get the [Block] to which this [BlockArgument] belongs
    pub fn owner(&self) -> BlockRef {
        self.owner.clone()
    }

    /// Get the index of this argument in the argument list of the owning [Block]
    pub fn index(&self) -> usize {
        self.index as usize
    }
}

impl crate::formatter::PrettyPrint for BlockArgument {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        text(format!("{}", self.id)) + const_text(": ") + self.ty.render()
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

impl OpResult {
    /// Get the [Operation] to which this [OpResult] belongs
    pub fn owner(&self) -> OperationRef {
        self.owner.clone()
    }

    /// Get the index of this result in the result list of the owning [Operation]
    pub fn index(&self) -> usize {
        self.index as usize
    }

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
