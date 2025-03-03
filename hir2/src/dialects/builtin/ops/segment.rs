use alloc::{collections::VecDeque, sync::Arc};
use core::fmt;

use midenc_hir_macros::operation;
use midenc_session::diagnostics::{miette, Diagnostic};

use crate::{
    constants::{ConstantData, ConstantId},
    dialects::builtin::BuiltinDialect,
    traits::*,
    Alignable, Op, OpPrinter, UnsafeIntrusiveEntityRef,
};

pub type SegmentRef = UnsafeIntrusiveEntityRef<Segment>;

/// Declare a data segment in the shared memory of a [Component].
///
/// This operation type is only permitted in the body of a [Module] op, it is an error to use it
/// anywhere else. At best it will be ignored.
///
/// Data segments can have a size that is larger than the initializer data it describes; in such
/// cases, the remaining memory is either assumed to be arbitrary bytes, or if `zeroed` is set,
/// it is zeroed so that the padding bytes are all zero.
///
/// A data segment can be marked `readonly`, which indicates to the optimizer that it is allowed
/// to assume that no writes will ever occur in the boundaries of the segment, i.e. a value loaded
/// from within those bounds does not need to be reloaded after side-effecting operations, and
/// can in fact be rescheduled around them. Additionally, if a write is detected that would effect
/// memory in a readonly data segment boundary, an error will be raised.
///
/// NOTE: It is not guaranteed that the optimizer will make any assumptions with regard to data
/// segments. For the moment, even if `readonly` is set, the compiler assumes that segments are
/// mutable.
#[operation(
    dialect = BuiltinDialect,
    traits(
        SingleBlock,
        NoRegionArguments,
        IsolatedFromAbove,
    ),
    implements(OpPrinter)
)]
pub struct Segment {
    /// The offset from the start of linear memory where this segment starts
    #[attr]
    offset: u32,
    /// The data to initialize this segment with, determines the size of the segment
    #[attr]
    data: ConstantId,
    /// Whether or not this segment is intended to be read-only data
    #[attr]
    #[default]
    readonly: bool,
}

impl Segment {
    /// The size, in bytes, of this data segment.
    ///
    /// By default this will be the same size as `init`, unless explicitly given.
    pub fn size_in_bytes(&self) -> usize {
        let id = *self.data();
        self.as_operation().context().get_constant_size_in_bytes(id)
    }

    /// Get the data, as bytes, to initialize this data segment with.
    pub fn initializer(&self) -> Arc<ConstantData> {
        let id = *self.data();
        self.as_operation().context().get_constant(id)
    }
}

impl fmt::Debug for Segment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let data = self.initializer();
        f.debug_struct("Segment")
            .field("offset", self.offset())
            .field("size", &data.len())
            .field("init", &format_args!("{data}"))
            .field("readonly", self.readonly())
            .finish()
    }
}

impl OpPrinter for Segment {
    fn print(
        &self,
        _flags: &crate::OpPrintingFlags,
        _context: &crate::Context,
    ) -> crate::formatter::Document {
        use crate::formatter::*;

        let header = display(self.op.name());
        let header = if *self.readonly() {
            header + const_text(" ") + const_text("readonly")
        } else {
            header
        };
        let header = header + const_text(" ") + text(format!("@{}", self.offset()));
        let data = self.initializer();
        header + const_text(" = ") + text(format!("0x{data};"))
    }
}

/// This error is raised when attempting to declare a [Segment] that in some way conflicts with
/// previously declared data segments.
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum DataSegmentError {
    /// The current segment overlaps with a previously allocated segment
    #[error(
        "invalid data segment: segment of {size1} bytes at {offset1:#x} overlaps with segment of \
         {size2} bytes at {offset2:#x}"
    )]
    #[diagnostic()]
    OverlappingSegments {
        offset1: u32,
        size1: u32,
        offset2: u32,
        size2: u32,
    },
    /// The current segment and a previous definition of that segment do
    /// not agree on the data or read/write properties of the memory they
    /// represent.
    #[error(
        "invalid data segment: segment at {0:#x} conflicts with a previous segment declaration at \
         this address"
    )]
    #[diagnostic()]
    Mismatch(u32),
    /// The current segment and size do not fall in the boundaries of the heap
    /// which is allocatable to globals and other heap allocations.
    ///
    /// For example, Miden reserves some amount of memory for procedure locals
    /// at a predetermined address, and we do not permit segments to be allocated
    /// past that point.
    #[error(
        "invalid data segment: segment of {size} bytes at {offset:#x} would extend beyond the end \
         of the usable heap"
    )]
    #[diagnostic()]
    OutOfBounds { offset: u32, size: u32 },
    /// The initializer for the current segment has a size greater than `u32::MAX` bytes
    #[error(
        "invalid data segment: segment at {0:#x} was declared with an initializer larger than \
         2^32 bytes"
    )]
    #[diagnostic()]
    InitTooLarge(u32),
    /// The initializer for the current segment has a size greater than the declared segment size
    #[error(
        "invalid data segment: segment of {size} bytes at {offset:#x} has an initializer of \
         {actual} bytes"
    )]
    #[diagnostic()]
    InitOutOfBounds { offset: u32, size: u32, actual: u32 },
}

/// This structure tracks a set of data segments to be placed in the same address space, and ensures
/// that the segments are laid out in that space without conflict.
#[derive(Default, Clone)]
pub struct DataSegmentLayout {
    segments: VecDeque<SegmentRef>,
}

impl DataSegmentLayout {
    /// Returns true if the table has no segments defined
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Returns the offset in linear memory where the last data segment ends
    pub fn next_available_offset(&self) -> u32 {
        if let Some(last_segment) = self.segments.back() {
            let last_segment = last_segment.borrow();
            let next_offset = *last_segment.offset() + last_segment.size_in_bytes() as u32;
            // Ensure the start of the next segment is word-aligned
            next_offset.align_up(32)
        } else {
            0
        }
    }

    /// Insert a [Segment] into the layout, while preserving the order of the segments.
    ///
    /// This will fail if the segment is invalid, or overlaps/conflicts with an existing segment.
    pub fn insert(&mut self, segment_ref: SegmentRef) -> Result<(), DataSegmentError> {
        if self.is_empty() {
            self.segments.push_back(segment_ref);
            return Ok(());
        }

        let segment = segment_ref.borrow();
        let offset = *segment.offset();
        let size = u32::try_from(segment.size_in_bytes())
            .map_err(|_| DataSegmentError::InitTooLarge(offset))?;
        let end = offset + size;
        for (index, current_segment_ref) in self.segments.iter().enumerate() {
            let current_segment = current_segment_ref.borrow();
            let current_offset = *current_segment.offset();
            let current_size = current_segment.size_in_bytes() as u32;
            let segment_end = current_offset + current_size;

            // If this segment starts after the segment we're declaring, we do not need to continue
            // searching for conflicts, and can go a head and perform the insert
            if current_offset >= end {
                self.segments.insert(index, segment_ref);
                return Ok(());
            }

            // If this segment starts at the same place as the one we're declaring that's a
            // guaranteed conflict
            if current_offset == offset {
                // If the two segments have the same size and offset, then
                // if they match in all other respects, we're done. If they
                // don't match, then we raise a mismatch error.
                if current_size == size
                    && current_segment.initializer() == segment.initializer()
                    && current_segment.readonly() == segment.readonly()
                {
                    return Ok(());
                }
                return Err(DataSegmentError::Mismatch(offset));
            }

            // This segment starts before the segment we're declaring, make sure that this segment
            // ends before our segment starts
            if segment_end > offset {
                return Err(DataSegmentError::OverlappingSegments {
                    offset1: offset,
                    size1: size,
                    offset2: current_offset,
                    size2: current_size,
                });
            }
        }

        self.segments.push_back(segment_ref);

        Ok(())
    }

    /// Traverse the data segments in the table in ascending order by offset
    pub fn iter(&self) -> impl Iterator<Item = SegmentRef> + '_ {
        self.segments.iter().copied()
    }

    /// Remove the first data segment from the table
    #[inline]
    pub fn pop_front(&mut self) -> Option<SegmentRef> {
        self.segments.pop_front()
    }
}

impl fmt::Debug for DataSegmentLayout {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut builder = f.debug_list();
        for segment in self.segments.iter() {
            let segment = segment.borrow();
            builder.entry(&segment);
        }
        builder.finish()
    }
}
