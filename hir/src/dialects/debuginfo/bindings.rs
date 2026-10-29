//! Semantic identities used by debug transforms. These are values, not audit snapshots.
use super::{
    DebugDeclare, DebugValue,
    attributes::{
        Expression, INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr, Variable,
    },
    debug_record_variable,
};
use crate::{Operation, OperationRef, interner::Symbol};

/// A source declaration in its record-owned inline frame. Recovery expressions are independent.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BindingIdentity {
    pub variable: Variable,
    pub inline_calls: Option<InlineCallChain>,
}

impl BindingIdentity {
    pub fn of(op: &Operation) -> Option<Self> {
        Some(Self {
            variable: debug_record_variable(op)?,
            inline_calls: op
                .get_typed_attribute::<InlineCallChainAttr>(INLINE_CALL_CHAIN_ATTR_NAME)
                .map(|chain| chain.borrow().as_value().clone()),
        })
    }

    pub fn visibility(&self, function: Option<OperationRef>) -> VisibilitySlot {
        VisibilitySlot {
            name: self.variable.name,
            inline_calls: self.inline_calls.clone(),
            function,
        }
    }
}

/// Declarations with the same visible name can shadow each other, even when their metadata differs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VisibilitySlot {
    name: Symbol,
    inline_calls: Option<InlineCallChain>,
    function: Option<OperationRef>,
}

impl VisibilitySlot {
    pub fn of(record: OperationRef) -> Option<Self> {
        BindingIdentity::of(&record.borrow())
            .map(|binding| binding.visibility(enclosing_function(record)))
    }
}

/// Full source identity across a root which may contain several functions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceBinding {
    pub identity: BindingIdentity,
    pub function: Option<OperationRef>,
}

impl SourceBinding {
    pub fn of(record: OperationRef) -> Option<Self> {
        Some(Self {
            identity: BindingIdentity::of(&record.borrow())?,
            function: enclosing_function(record),
        })
    }

    pub fn visibility(&self) -> VisibilitySlot {
        self.identity.visibility(self.function)
    }
}

/// Recovery facts are shareable only when both source identity and expression agree.
/// Consumers of this key must already be scoped to one function.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RecoveryIdentity {
    pub binding: BindingIdentity,
    pub expression: Option<Expression>,
}

impl RecoveryIdentity {
    pub fn of(op: &Operation) -> Option<Self> {
        Some(Self {
            binding: BindingIdentity::of(op)?,
            expression: debug_record_expression(op),
        })
    }
}

pub fn debug_record_expression(op: &Operation) -> Option<Expression> {
    if let Some(record) = op.downcast_ref::<DebugValue>() {
        Some(record.expression().as_value().clone())
    } else {
        op.downcast_ref::<DebugDeclare>()
            .map(|record| record.expression().as_value().clone())
    }
}

pub(super) fn enclosing_function(op: OperationRef) -> Option<OperationRef> {
    let mut current = Some(op);
    while let Some(operation) = current {
        if operation.name().is::<crate::dialects::builtin::Function>() {
            return Some(operation);
        }
        current = operation.parent_op();
    }
    None
}
