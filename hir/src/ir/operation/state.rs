use super::*;
use crate::{diagnostics::SourceSpan, interner};

/// This represents an operation in an abstracted form, suitable for use with the builder APIs.
///
/// This object is a large and heavy weight object meant to be used as a temporary object on the
/// stack.  It is generally unwise to put this in a collection.
pub struct OperationState {
    /// The operation being created
    pub name: OperationName,
    /// The source location associated with this op
    pub span: SourceSpan,
    /// The attributes to set on this operation
    pub attrs: SmallVec<[NamedAttribute; 1]>,
    /// The operands provided to this operation, broken up into groups as applicable
    pub operands: SmallVec<[ValueRef; 4]>,
    /// The types of the results of this operation
    pub results: SmallVec<[Type; 4]>,
    /// The regions that this op will hold
    pub regions: SmallVec<[RegionRef; 1]>,
    /// Successors of this operation
    pub successors: SmallVec<[BlockRef; 1]>,
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

    pub fn add_attribute(&mut self, name: impl Into<interner::Symbol>, value: AttributeRef) {
        self.attrs.push(NamedAttribute {
            name: name.into(),
            value,
        });
    }

    pub fn add_region(&mut self, region: RegionRef) {
        self.regions.push(region);
    }
}
