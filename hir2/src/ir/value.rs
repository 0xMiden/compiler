use core::fmt;

use super::*;

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

pub trait Value: Entity<Id = ValueId> + Spanned + Usable<Use = OpOperandImpl> + fmt::Debug {
    fn ty(&self) -> &Type;
    fn set_type(&mut self, ty: Type);
}

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

            #[inline]
            fn is_used(&self) -> bool {
                !self.uses.is_empty()
            }

            #[inline(always)]
            fn uses(&self) -> &OpOperandList {
                &self.uses
            }

            #[inline(always)]
            fn uses_mut(&mut self) -> &mut OpOperandList {
                &mut self.uses
            }

            #[inline]
            fn iter_uses(&self) -> OpOperandIter<'_> {
                self.uses.iter()
            }

            #[inline]
            fn first_use(&self) -> OpOperandCursor<'_> {
                self.uses.front()
            }

            #[inline]
            fn first_use_mut(&mut self) -> OpOperandCursorMut<'_> {
                self.uses.front_mut()
            }

            fn insert_use(&mut self, user: OpOperand) {
                self.uses.push_back(user);
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

pub type ValueRef = UnsafeEntityRef<dyn Value>;
pub type BlockArgumentRef = UnsafeEntityRef<BlockArgument>;
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
    pub fn owner(&self) -> BlockRef {
        self.owner.clone()
    }

    pub fn index(&self) -> usize {
        self.index as usize
    }
}

impl OpResult {
    pub fn owner(&self) -> OperationRef {
        self.owner.clone()
    }

    pub fn index(&self) -> usize {
        self.index as usize
    }
}

pub type OpOperand = UnsafeIntrusiveEntityRef<OpOperandImpl>;
pub type OpOperandList = EntityList<OpOperandImpl>;
pub type OpOperandIter<'a> = EntityIter<'a, OpOperandImpl>;
pub type OpOperandCursor<'a> = EntityCursor<'a, OpOperandImpl>;
pub type OpOperandCursorMut<'a> = EntityCursorMut<'a, OpOperandImpl>;

/// An [OpOperand] represents a use of a [Value] by an [Operation]
pub struct OpOperandImpl {
    /// The operand value
    pub value: ValueRef,
    /// The owner of this operand, i.e. the operation it is an operand of
    pub owner: OperationRef,
    /// The index of this operand in the operand list of an operation
    pub index: u8,
}
impl OpOperandImpl {
    #[inline]
    pub fn new(value: ValueRef, owner: OperationRef, index: u8) -> Self {
        Self {
            value,
            owner,
            index,
        }
    }

    pub fn value(&self) -> EntityRef<'_, dyn Value> {
        self.value.borrow()
    }
}
impl fmt::Debug for OpOperandImpl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        #[allow(unused)]
        struct ValueInfo<'a> {
            id: ValueId,
            ty: &'a Type,
        }

        let value = self.value.borrow();
        let id = value.id();
        let ty = value.ty();
        f.debug_struct("OpOperand")
            .field("index", &self.index)
            .field("value", &ValueInfo { id, ty })
            .finish_non_exhaustive()
    }
}

pub enum OpOperandValue {
    Value(ValueRef),
    Immediate(Immediate),
}
