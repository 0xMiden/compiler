use super::*;
use crate::{attributes::IntoAttributeRef, diagnostics::SourceSpan, interner, smallvec};

/// This represents an operation in an abstracted form, suitable for use with the builder APIs.
///
/// This object is a large and heavy weight object meant to be used as a temporary object on the
/// stack.  It is generally unwise to put this in a collection.
#[derive(Debug)]
pub struct OperationState {
    /// The operation being created
    pub name: OperationName,
    /// The source location associated with this op
    pub span: SourceSpan,
    /// The attributes to set on this operation
    pub attrs: SmallVec<[NamedAttribute; 1]>,
    /// The operands provided to this operation, broken up into groups as applicable
    pub operands: SmallVec<[SmallVec<[ValueRef; 2]>; 1]>,
    /// The types of the results of this operation
    pub results: SmallVec<[Type; 4]>,
    /// The regions that this op will hold
    pub regions: SmallVec<[RegionRef; 1]>,
    /// Successors of this operation
    pub successors: SmallVec<[PendingSuccessorInfo; 1]>,
}

/// A successor recorded by [OperationState] before the operation is built
#[derive(Debug)]
pub struct PendingSuccessorInfo {
    /// The destination block
    pub block: BlockRef,
    /// The key selecting this successor, for keyed successors such as the cases of a switch
    pub key: Option<AttributeRef>,
    /// The index of the operand group holding the arguments forwarded to `block`
    pub operand_group: u16,
    /// The successor group this successor belongs to, matching the grouping expected by the
    /// operation's successor accessors (e.g. a `cf.switch`'s cases vs. its fallback).
    pub successor_group: u16,
}

impl OperationState {
    pub fn new(span: SourceSpan, name: OperationName) -> Self {
        Self {
            name,
            span,
            attrs: Default::default(),
            operands: Default::default(),
            results: Default::default(),
            regions: Default::default(),
            successors: Default::default(),
        }
    }

    pub fn add_operand(&mut self, operand: ValueRef) {
        if let Some(group) = self.operands.last_mut() {
            group.push(operand);
        } else {
            self.operands.push(smallvec![operand]);
        }
    }

    pub fn add_operands(&mut self, operands: SmallVec<[ValueRef; 2]>) {
        self.operands.push(operands);
    }

    pub fn add_attribute(
        &mut self,
        name: impl Into<interner::Symbol>,
        value: impl IntoAttributeRef,
    ) {
        self.attrs.push(NamedAttribute {
            name: name.into(),
            value: value.into_attribute_ref(),
        });
    }

    pub fn add_region(&mut self, region: RegionRef) {
        self.regions.push(region);
    }

    /// Add `block` as a successor in `successor_group`, with its arguments in `operand_group`
    pub fn add_successor(&mut self, block: BlockRef, operand_group: u16, successor_group: u16) {
        self.successors.push(PendingSuccessorInfo {
            block,
            key: None,
            operand_group,
            successor_group,
        });
    }

    /// Add `block` as a successor selected by `key`, e.g. one case of a switch
    pub fn add_keyed_successor(
        &mut self,
        key: AttributeRef,
        block: BlockRef,
        operand_group: u16,
        successor_group: u16,
    ) {
        self.successors.push(PendingSuccessorInfo {
            block,
            key: Some(key),
            operand_group,
            successor_group,
        });
    }
}
