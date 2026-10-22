//! Validated raw value layouts and explicitly declared lane equalities.
use alloc::vec::Vec;

use midenc_hir::Type;

use super::{Atom, DiagnosticCategory, PlanError};

/// Validated raw representation of one ordinary operand, occupying one to four lanes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValueLayout {
    ty: Type,
    raw: Vec<Type>,
}

impl ValueLayout {
    /// Reject zero-sized, unrepresentable, or wider-than-word operands before planning.
    pub fn new(ty: Type) -> Result<Self, PlanError> {
        let raw = ty.clone().to_raw_parts().ok_or_else(|| {
            PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "operand has no raw representation",
            )
        })?;
        if ty.size_in_bytes() == 0 || raw.is_empty() || raw.len() > miden_core::WORD_SIZE {
            return Err(PlanError::new(
                DiagnosticCategory::InvalidSelectedLowering,
                "ordinary operands must have one to four raw lanes",
            ));
        }
        Ok(Self {
            ty,
            raw: raw.into_iter().collect(),
        })
    }

    /// Semantic type whose raw representation was validated at construction.
    pub fn ty(&self) -> &Type {
        &self.ty
    }

    /// Lane types in stack order, with the first lane nearest the top.
    #[cfg(test)]
    pub fn raw_parts(&self) -> &[Type] {
        &self.raw
    }

    /// Number of physical stack elements needed for one copy of this value.
    pub fn lanes(&self) -> u8 {
        self.raw.len() as u8
    }
}

/// Ordered test operand layouts; their combined width may exceed one ordinary value.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TupleLayout(Vec<ValueLayout>);
#[cfg(test)]
impl TupleLayout {
    /// Preserve semantic tuple order without imposing an aggregate lane limit.
    pub fn new(values: Vec<ValueLayout>) -> Self {
        Self(values)
    }

    /// Total lane count after flattening the ordered values.
    pub fn lanes(&self) -> usize {
        self.0.iter().map(|v| usize::from(v.lanes())).sum()
    }
}

/// Proven equalities between lanes, used by selected units and entry contracts.
/// An alias permits another semantic name for the same contents; it creates no physical copy.
/// Selected-unit declarations pair an output with its source; entry contracts may instead
/// relate existing live lanes without assigning either one a direction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LaneAliases(Vec<(Atom, Atom)>);

impl LaneAliases {
    /// Canonicalize relation order and exact duplicates; this does not prove the equalities.
    pub fn new(mut pairs: Vec<(Atom, Atom)>) -> Self {
        pairs.sort_unstable();
        pairs.dedup();
        Self(pairs)
    }

    /// Find the source named for an output lane, if this relation includes it.
    pub fn source(&self, output: Atom) -> Option<Atom> {
        self.0.iter().find(|(result, _)| *result == output).map(|(_, source)| *source)
    }

    /// Canonical equality pairs for validation and contract replay.
    pub fn pairs(&self) -> &[(Atom, Atom)] {
        &self.0
    }
}
