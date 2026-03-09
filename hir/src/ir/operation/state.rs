use super::*;
use crate::{diagnostics::SourceSpan, interner, smallvec};

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

#[derive(Debug)]
pub struct PendingSuccessorInfo {
    pub block: BlockRef,
    pub key: Option<AttributeRef>,
    pub operand_group: u8,
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

    pub fn add_attribute(&mut self, name: impl Into<interner::Symbol>, value: AttributeRef) {
        self.attrs.push(NamedAttribute {
            name: name.into(),
            value,
        });
    }

    pub fn add_region(&mut self, region: RegionRef) {
        self.regions.push(region);
    }

    pub fn add_successor(&mut self, block: BlockRef, operand_group: u8) {
        self.successors.push(PendingSuccessorInfo {
            block,
            key: None,
            operand_group,
        });
    }

    pub fn add_keyed_successor(&mut self, key: AttributeRef, block: BlockRef, operand_group: u8) {
        self.successors.push(PendingSuccessorInfo {
            block,
            key: Some(key),
            operand_group,
        });
    }
}
