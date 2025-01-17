use cranelift_entity::{self as entity, entity_impl};

use crate::{diagnostics::SourceSpan, Block, Inst, Type};

pub type ValueList = entity::EntityList<Value>;
pub type ValueListPool = entity::ListPool<Value>;

/// A handle to a single SSA value
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Value(u32);
entity_impl!(Value, "v");
impl Default for Value {
    #[inline]
    fn default() -> Self {
        use cranelift_entity::packed_option::ReservedValue;

        Self::reserved_value()
    }
}

/// Data associated with a `Value`.
///
/// Values are either block arguments, instructions or aliases, and
/// in addition to being linked to a `Inst` or a `Block`, they
/// have an associated type, position, and in some cases, a `SourceSpan`.
#[derive(Debug, Clone)]
pub enum ValueData {
    Inst {
        ty: Type,
        num: u16,
        inst: Inst,
    },
    Param {
        ty: Type,
        num: u16,
        block: Block,
        span: SourceSpan,
    },
}
impl ValueData {
    pub fn ty(&self) -> &Type {
        match self {
            Self::Inst { ref ty, .. } | Self::Param { ref ty, .. } => ty,
        }
    }

    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Inst { .. } => SourceSpan::UNKNOWN,
            Self::Param { span, .. } => *span,
        }
    }

    /// Update the block to which a block parameter belongs
    ///
    /// NOTE: This function will panic if the value is not a block parameter
    ///
    /// # Safety
    ///
    /// This function is marked unsafe because changing the block associated
    /// with a value could cause unexpected results if the other pieces of the
    /// DataFlowGraph are not updated correctly. Callers must ensure that this
    /// is _only_ called when a block parameter has been moved to another block.
    pub unsafe fn set_block(&mut self, block: Block) {
        match self {
            Self::Param {
                block: ref mut orig,
                ..
            } => {
                *orig = block;
            }
            _ => panic!("expected block parameter, got instruction result"),
        }
    }

    pub fn set_type(&mut self, ty: Type) {
        match self {
            Self::Inst {
                ty: ref mut prev_ty,
                ..
            } => *prev_ty = ty,
            Self::Param {
                ty: ref mut prev_ty,
                ..
            } => *prev_ty = ty,
        }
    }

    pub fn unwrap_inst(&self) -> Inst {
        match self {
            Self::Inst { inst, .. } => *inst,
            _ => panic!("expected instruction result value, got block parameter"),
        }
    }

    pub fn num(&self) -> u16 {
        match self {
            Self::Inst { num, .. } | Self::Param { num, .. } => *num,
        }
    }
}

pub struct Values<'a> {
    pub(super) inner: entity::Iter<'a, Value, ValueData>,
}
impl Iterator for Values<'_> {
    type Item = Value;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.by_ref().next().map(|kv| kv.0)
    }
}
