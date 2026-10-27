//! Pure source frame and inline-chain descriptions shared by selection and debug metadata.
use midenc_hir::{Operation, dialects::debuginfo::attributes::INLINE_CALL_CHAIN_ATTR_NAME};

/// The layout of a procedure's locals frame, in field elements.
///
/// Debug locations refer to locals by index, while the frame is addressed by element offset, so
/// both are needed to place a local relative to the frame pointer.
#[cfg(any(test, doc))]
#[derive(Clone, Copy, Default)]
pub(crate) struct FrameLayout<'a> {
    /// The offset of each local from the start of the frame, indexed by local.
    ///
    /// A local wider than one element moves every local declared after it, so the index of a local
    /// is not its offset.
    pub local_offsets: &'a [u32],
    /// The size of the frame, rounded up the way the assembler rounds it. `locaddr.N` addresses
    /// `FMP - aligned_size + N`, so offsets relative to the frame pointer are derived from it.
    pub aligned_size: u32,
}

#[cfg(any(test, doc))]
impl<'a> FrameLayout<'a> {
    /// Builds the layout of a frame holding `num_locals` elements, with `local_offsets` from
    /// [`Function::local_offsets`](midenc_hir::dialects::builtin::Function::local_offsets).
    pub fn new(local_offsets: &'a [u32], num_locals: u16) -> Self {
        Self {
            local_offsets,
            aligned_size: u32::from(num_locals).next_multiple_of(miden_core::WORD_SIZE as u32),
        }
    }

    /// The element offset shared by executable local accesses and debug expressions.
    pub fn element_offset(&self, index: usize) -> Option<u32> {
        self.local_offsets.get(index).copied()
    }
}

#[cfg(any(test, doc))]
impl<'a> From<&'a crate::plan::FinalFrameLayout> for FrameLayout<'a> {
    fn from(frame: &'a crate::plan::FinalFrameLayout) -> Self {
        Self {
            local_offsets: &frame.source().offsets,
            aligned_size: frame.aligned_size(),
        }
    }
}

pub(crate) fn has_inline_call_chain(operation: &Operation) -> bool {
    let mut has_inline_call_chain = false;
    operation.prewalk_all(|op| {
        has_inline_call_chain |= op.has_attribute(INLINE_CALL_CHAIN_ATTR_NAME);
    });
    has_inline_call_chain
}
