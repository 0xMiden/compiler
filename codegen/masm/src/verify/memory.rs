//! Bounds, initialization, and exact-content checks for the independently replayed frame.
use alloc::collections::BTreeMap;

use super::{
    VerificationError, fail,
    stack::State,
    tokens::{Token, Tokens},
};
use crate::plan::*;

/// Read a physical cell only if it lies in the frame and is definitely initialized.
pub(super) fn read(
    state: &State,
    offset: u32,
    frame: &FinalFrameLayout,
) -> Result<Token, VerificationError> {
    check_range(offset, 1, frame)?;
    state.cells.get(&offset).copied().ok_or_else(|| {
        fail(
            DiagnosticCategory::InvalidSpillSlotRead,
            "physical cell is not definitely initialized",
        )
    })
}

/// Check an element-addressed interval against the final frame, including arithmetic overflow.
pub(super) fn check_range(
    offset: u32,
    width: u32,
    frame: &FinalFrameLayout,
) -> Result<(), VerificationError> {
    if offset.checked_add(width).is_none_or(|end| end > frame.aligned_size()) {
        return Err(fail(
            DiagnosticCategory::FrameCapacityExceeded,
            "physical access outside final frame",
        ));
    }
    Ok(())
}

/// Resolve a nonempty object-relative scratch interval, rejecting undeclared objects,
/// object overflow, or any range exceeding the assigned frame.
pub(super) fn object_range(
    object: ScratchId,
    offset: u32,
    size: u32,
    frame: &FinalFrameLayout,
) -> Result<core::ops::Range<u32>, VerificationError> {
    let allocation = frame.scratch().get(&object).ok_or_else(|| {
        fail(DiagnosticCategory::InvalidSpillSlotRead, "access to undeclared scratch object")
    })?;
    if size == 0 || offset.checked_add(size).is_none_or(|end| end > allocation.size) {
        return Err(fail(
            DiagnosticCategory::InvalidSpillSlotRead,
            "access exceeds scratch object",
        ));
    }
    let start = allocation.offset.checked_add(offset).ok_or_else(|| {
        fail(DiagnosticCategory::FrameCapacityExceeded, "scratch address overflow")
    })?;
    let end =
        start
            .checked_add(size)
            .filter(|end| *end <= frame.aligned_size())
            .ok_or_else(|| {
                fail(DiagnosticCategory::FrameCapacityExceeded, "scratch range exceeds frame")
            })?;
    Ok(start..end)
}

/// A possible felt write destroys identity, never existing definite initialization.
/// A definite write also establishes initialization of previously empty cells.
pub(super) fn invalidate(
    state: &mut State,
    range: core::ops::Range<u32>,
    definite: bool,
    tokens: &mut Tokens,
) {
    for cell in range {
        if definite || state.cells.contains_key(&cell) {
            state.cells.insert(cell, tokens.fresh());
        }
        if definite {
            state.written.insert(cell);
        }
    }
}

/// Discharge a scratch obligation against actual physical contents, comparing exact tokens
/// for atom/content requirements and only initialization for the weakest requirement.
pub(super) fn check_fact(
    state: &State,
    fact: ScratchCellFact,
    frame: &FinalFrameLayout,
    contents: &BTreeMap<ContentId, Token>,
) -> Result<(), VerificationError> {
    let actual = read(
        state,
        frame
            .resolve(FrameRef::Scratch {
                object: fact.object,
                offset: fact.offset,
            })?
            .into(),
        frame,
    )?;
    let expected = match fact.content {
        ContentRequirement::Initialized => return Ok(()),
        ContentRequirement::Atom(atom) => state.token(atom)?,
        ContentRequirement::Content(id) => *contents.get(&id).ok_or_else(|| {
            fail(
                DiagnosticCategory::InvalidSpillSlotRead,
                "scratch content has no verified definition",
            )
        })?,
    };
    if actual != expected {
        return Err(fail(
            DiagnosticCategory::InvalidSpillSlotRead,
            "physical scratch content differs from required identity",
        ));
    }
    Ok(())
}

/// Obtain the expected identity of a stored input without assuming it occupies any cell.
pub(super) fn stored_token(
    state: &State,
    value: StoredValue,
    tokens: &mut Tokens,
) -> Result<Token, VerificationError> {
    match value {
        StoredValue::Atom(atom) => state.token(atom),
        StoredValue::Literal(value) => Ok(tokens.literal(value)),
    }
}
