//! Module for handling data segment alignment and merging for Miden VM
//!
//! Miden VM requires all data segments to be word-aligned (16-byte aligned) because the
//! hash/unhash instructions operate on words when loaded from the advice provider. This module provides
//! functionality to ensure all data segments meet this requirement.

use midenc_hir::SmallVec;

/// Represents a data segment with resolved offset
#[derive(Debug, Clone)]
pub struct ResolvedDataSegment {
    /// The absolute offset in linear memory where this segment starts
    pub offset: u32,
    /// The initialization data
    pub data: Vec<u8>,
    /// The original name/index for debugging purposes
    pub name: String,
    /// Whether this is readonly data
    pub readonly: bool,
}

impl ResolvedDataSegment {
    /// Check if this segment is word-aligned (16-byte aligned)
    pub fn is_word_aligned(&self) -> bool {
        self.offset.is_multiple_of(16)
    }

    /// Calculate padding needed to align this segment to word boundary
    /// This returns how many bytes we need to prepend when moving offset down to word boundary
    pub fn padding_needed(&self) -> u32 {
        self.offset % 16
    }

    /// Align this segment to word boundary by adjusting offset and prepending zeros
    pub fn align_to_word_boundary(&mut self) {
        let padding = self.padding_needed();
        if padding > 0 {
            // Adjust offset down to nearest word boundary
            self.offset -= padding;
            // Prepend zeros to maintain data at correct offset
            let mut new_data = Vec::with_capacity(padding as usize + self.data.len());
            new_data.resize(padding as usize, 0);
            new_data.extend_from_slice(&self.data);
            self.data = new_data;
        }
    }

    /// Check if this segment overlaps with another segment
    pub fn overlaps_with(&self, other: &ResolvedDataSegment) -> bool {
        let self_end = self.offset + self.data.len() as u32;
        let other_end = other.offset + other.data.len() as u32;

        (self.offset < other_end) && (other.offset < self_end)
    }

    /// Merge this segment with another segment, filling gap with zeros
    pub fn merge_with(&mut self, other: ResolvedDataSegment) {
        // Ensure this segment comes before the other
        assert!(self.offset <= other.offset, "Segments must be sorted by offset");

        let self_end = self.offset + self.data.len() as u32;
        let gap = other.offset.saturating_sub(self_end);

        // Extend data to include gap (filled with zeros) and other segment's data
        self.data.reserve((gap + other.data.len() as u32) as usize);
        self.data.extend(vec![0u8; gap as usize]);
        self.data.extend(other.data);

        // Update name to indicate merge
        self.name = format!("{}_merged_{}", self.name, other.name);
        // Merged segment is readonly only if both segments were readonly
        self.readonly = self.readonly && other.readonly;
    }
}

/// Process data segments to ensure word alignment
///
/// This function takes a collection of data segments and ensures they are all
/// word-aligned, merging segments if necessary to avoid overlaps.
pub fn align_data_segments(
    segments: SmallVec<[ResolvedDataSegment; 2]>,
) -> SmallVec<[ResolvedDataSegment; 2]> {
    if segments.is_empty() {
        return segments;
    }

    // Sort segments by offset
    let mut sorted_segments = segments;
    sorted_segments.sort_by_key(|s| s.offset);

    let mut result = SmallVec::<[ResolvedDataSegment; 2]>::new();
    let mut current_segment = sorted_segments[0].clone();

    for next_segment in sorted_segments.into_iter().skip(1) {
        let mut aligned_next = next_segment.clone();
        aligned_next.align_to_word_boundary();

        // Check if aligned segment would overlap with current segment
        if current_segment.overlaps_with(&aligned_next) {
            // Merge segments
            current_segment.merge_with(aligned_next);
        } else {
            // No overlap, align current segment and add to result
            current_segment.align_to_word_boundary();
            result.push(current_segment);
            current_segment = aligned_next;
        }
    }

    // Don't forget the last segment
    current_segment.align_to_word_boundary();
    result.push(current_segment);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_word_aligned() {
        let segment = ResolvedDataSegment {
            offset: 0,
            data: vec![1, 2, 3, 4],
            name: "test".to_string(),
            readonly: true,
        };
        assert!(segment.is_word_aligned());

        let segment = ResolvedDataSegment {
            offset: 16,
            data: vec![1, 2, 3, 4],
            name: "test".to_string(),
            readonly: true,
        };
        assert!(segment.is_word_aligned());

        let segment = ResolvedDataSegment {
            offset: 12,
            data: vec![1, 2, 3, 4],
            name: "test".to_string(),
            readonly: true,
        };
        assert!(!segment.is_word_aligned());
    }

    #[test]
    fn test_padding_needed() {
        let segment = ResolvedDataSegment {
            offset: 0,
            data: vec![1, 2, 3, 4],
            name: "test".to_string(),
            readonly: true,
        };
        assert_eq!(segment.padding_needed(), 0);

        let segment = ResolvedDataSegment {
            offset: 12,
            data: vec![1, 2, 3, 4],
            name: "test".to_string(),
            readonly: true,
        };
        assert_eq!(segment.padding_needed(), 12);

        let segment = ResolvedDataSegment {
            offset: 1048620, // The problematic offset from p2id example
            data: vec![1, 2, 3, 4],
            name: "test".to_string(),
            readonly: true,
        };
        assert_eq!(segment.padding_needed(), 12);
    }

    #[test]
    fn test_align_to_word_boundary() {
        let mut segment = ResolvedDataSegment {
            offset: 12,
            data: vec![1, 2, 3, 4],
            name: "test".to_string(),
            readonly: true,
        };
        segment.align_to_word_boundary();

        assert_eq!(segment.offset, 0);
        // Should prepend 12 zeros since we moved offset from 12 to 0
        assert_eq!(segment.data, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_overlaps_with() {
        let segment1 = ResolvedDataSegment {
            offset: 0,
            data: vec![0; 20],
            name: "seg1".to_string(),
            readonly: true,
        };

        let segment2 = ResolvedDataSegment {
            offset: 16,
            data: vec![0; 20],
            name: "seg2".to_string(),
            readonly: true,
        };

        assert!(segment1.overlaps_with(&segment2));

        let segment3 = ResolvedDataSegment {
            offset: 20,
            data: vec![0; 10],
            name: "seg3".to_string(),
            readonly: true,
        };

        assert!(!segment1.overlaps_with(&segment3));
    }

    #[test]
    fn test_merge_segments() {
        let mut segment1 = ResolvedDataSegment {
            offset: 0,
            data: vec![1, 2, 3, 4],
            name: "seg1".to_string(),
            readonly: true,
        };

        let segment2 = ResolvedDataSegment {
            offset: 8,
            data: vec![5, 6, 7, 8],
            name: "seg2".to_string(),
            readonly: true,
        };

        segment1.merge_with(segment2);

        assert_eq!(segment1.offset, 0);
        assert_eq!(segment1.data, vec![1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8]);
        assert_eq!(segment1.name, "seg1_merged_seg2");
    }

    #[test]
    fn test_align_data_segments_p2id_case() {
        // Simulate the p2id case
        let segments = [
            ResolvedDataSegment {
                offset: 1048576,   // 0x100000 - already aligned
                data: vec![0; 44], // Size of the .rodata string
                name: ".rodata".to_string(),
                readonly: true,
            },
            ResolvedDataSegment {
                offset: 1048620,   // 0x10002C - not aligned
                data: vec![0; 76], // Size of the .data segment
                name: ".data".to_string(),
                readonly: false,
            },
        ];

        let aligned = align_data_segments(segments.into());

        // Should merge into one segment since alignment causes overlap
        assert_eq!(aligned.len(), 1);
        assert!(aligned[0].is_word_aligned());
        assert_eq!(aligned[0].offset, 1048576);
        // .rodata size (44) + gap to .data start (44) + .data padding (12) + .data size (76) = 132
        // Gap: 1048620 - (1048576 + 44) = 0
        // But .data aligns to 1048608, so it overlaps with end of .rodata
        // So we get: .rodata (44) + merged .data starting at original position with 12 byte padding + data (76) = 132
        assert_eq!(aligned[0].data.len(), 132);
        assert!(!aligned[0].readonly, "resulted merged segment should not be readonly");
    }
}
