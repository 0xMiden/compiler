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
    width: u8,
    frame: &FinalFrameLayout,
) -> Result<(), VerificationError> {
    if offset.checked_add(width.into()).is_none_or(|end| end > frame.aligned_size()) {
        return Err(fail(
            DiagnosticCategory::FrameCapacityExceeded,
            "physical access outside final frame",
        ));
    }
    Ok(())
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
