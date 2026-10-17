//! Independent raw state. Protected caller lanes never become ordinary owned occurrences.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};

use super::{
    VerificationError, fail,
    results::{Requirement, Results},
    tokens::{Identity, Token, Tokens},
};
use crate::plan::{Atom, DiagnosticCategory, FloorStep};

/// One representative for each possible caller suffix length up to the VM floor. Lengths
/// beyond sixteen are equivalent for floor effects because instructions cannot touch the suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Padding(pub [u32; 17]);
impl Padding {
    /// Initialize the padding count for every representative caller-suffix length.
    pub fn new(owned: usize) -> Self {
        Self(core::array::from_fn(|suffix| 1u32 << 16usize.saturating_sub(owned + suffix)))
    }

    /// Add floor padding forced by a pop at this owned height, preserving prior possibilities.
    pub fn floor(&mut self, owned: usize) {
        for (suffix, padding) in self.0.iter_mut().enumerate() {
            let floor = 16usize.saturating_sub(owned + suffix);
            *padding = (0..=16)
                .filter(|count| *padding & (1 << count) != 0)
                .fold(0, |bits, count| bits | (1 << count.max(floor)));
        }
    }

    /// Retain every padding count possible on either incoming path.
    pub fn union(&mut self, other: &Self) {
        for (padding, other) in self.0.iter_mut().zip(other.0) {
            *padding |= other;
        }
    }
}

/// Raw state owned by replay, with semantic names kept separate from physical availability.
/// A token may still be named after all of its stack/cell representations have been destroyed.
#[derive(Debug, Clone)]
pub(super) struct State {
    /// Owned raw lanes, nearest stack top first; the protected caller suffix is implicit.
    pub stack: Vec<Token>,
    /// Definitely initialized physical frame cells and their current identities.
    pub cells: BTreeMap<u32, Token>,
    /// Semantic identities already established by entry, transfer, or unit publication.
    pub names: BTreeMap<Atom, Token>,
    /// Cells definitely written during the current unit, intersected across branches.
    pub written: BTreeSet<u32>,
    /// Path-sensitive call-result equalities still required at unit publication.
    pub results: Results,
    /// Independently named scratch-content identities, distinct from the physical cells.
    pub contents: BTreeMap<crate::plan::ContentId, Token>,
    /// Logical spill-slot identities used to detect reads after physical cell reuse.
    pub slots: BTreeMap<crate::plan::SpillSlotId, Token>,
    pub padding: Padding,
}
impl State {
    /// Start with the given owned prefix and names, without assuming initialized frame storage.
    pub fn new(stack: Vec<Token>, names: BTreeMap<Atom, Token>) -> Self {
        let padding = Padding::new(stack.len());
        Self {
            stack,
            names,
            written: BTreeSet::new(),
            results: BTreeMap::new(),
            contents: BTreeMap::new(),
            cells: BTreeMap::new(),
            slots: BTreeMap::new(),
            padding,
        }
    }

    /// Resolve an established semantic identity or report an unavailable value.
    pub fn token(&self, atom: Atom) -> Result<Token, VerificationError> {
        self.names.get(&atom).copied().ok_or_else(|| {
            fail(DiagnosticCategory::UnavailableValue, "semantic atom has not been established")
        })
    }

    /// Borrow an owned prefix, rejecting access into caller lanes or VM padding.
    pub fn prefix(&self, count: usize) -> Result<&[Token], VerificationError> {
        self.stack.get(..count).ok_or_else(|| {
            fail(
                DiagnosticCategory::IllegalStackAccess,
                "raw access crosses protected caller suffix or floor padding",
            )
        })
    }

    /// Remove one owned lane and model any VM floor padding introduced by that pop.
    pub fn pop(&mut self) -> Result<Token, VerificationError> {
        let token = self.stack.first().copied().ok_or_else(|| {
            fail(DiagnosticCategory::IllegalStackAccess, "pop crosses protected caller suffix")
        })?;
        self.stack.remove(0);
        self.padding.floor(self.stack.len());
        Ok(token)
    }

    /// Apply a trusted prefix transition and its actual temporary height changes. Reject
    /// suffix crossings or disagreement between the height trace and final lane counts.
    pub fn replace(
        &mut self,
        consumed: usize,
        outputs: Vec<Token>,
        floor: &[FloorStep],
    ) -> Result<(), VerificationError> {
        self.prefix(consumed)?;
        let mut owned = self.stack.len();
        for step in floor {
            match *step {
                FloorStep::Push(count) => {
                    owned = owned.checked_add(count.into()).ok_or_else(|| {
                        fail(DiagnosticCategory::IllegalStackAccess, "stack height overflow")
                    })?
                }
                FloorStep::Pop(count) => {
                    owned = owned.checked_sub(count.into()).ok_or_else(|| {
                        fail(
                            DiagnosticCategory::IllegalStackAccess,
                            "temporary pop crosses protected suffix",
                        )
                    })?;
                    self.padding.floor(owned);
                }
            }
        }
        if owned != self.stack.len() - consumed + outputs.len() {
            return Err(fail(
                DiagnosticCategory::IllegalStackAccess,
                "target floor rule and lane effect disagree",
            ));
        }
        self.stack.splice(..consumed, outputs);
        Ok(())
    }

    /// Whether a token has a physical copy on stack or in one of the supplied spill cells.
    pub fn available(&self, token: Token, spill_offsets: impl Iterator<Item = u32>) -> bool {
        self.stack.contains(&token)
            || spill_offsets.into_iter().any(|offset| self.cells.get(&offset) == Some(&token))
    }

    /// Join equal-shaped branches with path-correlated tokens. Keep only definitely present
    /// names/cells/slots, intersect definite writes, and preserve conditional result obligations.
    pub fn merge(
        &self,
        other: &Self,
        id: crate::plan::ControlId,
        tokens: &mut Tokens,
    ) -> Result<Self, VerificationError> {
        if self.stack.len() != other.stack.len() {
            return Err(fail(
                DiagnosticCategory::IllegalStackAccess,
                "branch owned stack shapes disagree",
            ));
        }
        let scope = tokens.scope();
        let mut join = |a, b| {
            if a == b {
                a
            } else {
                tokens.intern(Identity::Join(scope, id, alloc::vec![a, b]))
            }
        };
        let stack = self.stack.iter().zip(&other.stack).map(|(a, b)| join(*a, *b)).collect();
        let cells = self
            .cells
            .iter()
            .filter_map(|(offset, a)| other.cells.get(offset).map(|b| (*offset, join(*a, *b))))
            .collect();
        // Semantic names are external obligations, not branch-local renaming opportunities.
        let names = self
            .names
            .iter()
            .filter_map(|(atom, a)| other.names.get(atom).map(|b| (*atom, join(*a, *b))))
            .collect();
        let slots = self
            .slots
            .iter()
            .filter_map(|(slot, a)| other.slots.get(slot).map(|b| (*slot, join(*a, *b))))
            .collect();
        let mut padding = self.padding.clone();
        padding.union(&other.padding);
        let contents = self
            .contents
            .iter()
            .filter_map(|(id, a)| other.contents.get(id).map(|b| (*id, join(*a, *b))))
            .collect();
        let results = Requirement::merge(scope, &self.results, &other.results);
        let written = self.written.intersection(&other.written).copied().collect();
        Ok(Self {
            stack,
            cells,
            names,
            slots,
            contents,
            results,
            written,
            padding,
        })
    }
}
