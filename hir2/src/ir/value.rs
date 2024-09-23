use core::fmt;

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
    /// Get the type of this value
    fn ty(&self) -> &Type;
    /// Set the type of this value
    fn set_type(&mut self, ty: Type);
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
                id: ValueId,
                ty: Type,
                $(
                    $Field: $FieldTy
                ),*
            ) -> Self {
                Self {
                    id,
                    ty,
                    span: Default::default(),
                    uses: Default::default(),
                    $(
                        $Field
                    ),*
                }
            }
        }

        impl Value for $ValueKind {
            fn ty(&self) -> &Type {
                &self.ty
            }

            fn set_type(&mut self, ty: Type) {
                self.ty = ty;
            }
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
);

value_impl!(
    /// An [OpResult] represents the definition of a [Value] by the result of an [Operation]
    pub struct OpResult {
        owner: OperationRef,
        index: u8,
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

impl OpResult {
    /// Get the [Operation] to which this [OpResult] belongs
    pub fn owner(&self) -> OperationRef {
        self.owner.clone()
    }

    /// Get the index of this result in the result list of the owning [Operation]
    pub fn index(&self) -> usize {
        self.index as usize
    }
}
