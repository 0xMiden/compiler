//! Module for handling data segment merging for Miden VM
//!
//! Miden VM requires data to be word-aligned (16-byte aligned) for hash/unhash instructions.
//! This module merges all data segments into a single segment and pads to 16-byte alignment.

use std::fmt::Debug;

use miden_core::utils::ToHex;
use midenc_hir::SmallVec;
use midenc_session::diagnostics::Report;

/// Word size in bytes for Miden VM alignment requirements
const WORD_SIZE_BYTES: usize = 16;

/// Represents a data segment with resolved offset
#[derive(Clone)]
pub struct ResolvedDataSegment {
    /// The absolute offset in linear memory where this segment starts
    pub offset: u32,
    /// The initialization data
    pub data: Vec<u8>,
    /// Whether this is readonly data
    pub readonly: bool,
}

impl ResolvedDataSegment {
    /// Returns the end offset of this segment
    fn end_offset(&self) -> u32 {
        self.offset + self.data.len() as u32
    }

    /// Pads the segment data to be aligned to WORD_SIZE_BYTES (16 bytes)
    pub fn pad_to_word_alignment(&mut self) {
        let remainder = self.data.len() % WORD_SIZE_BYTES;
        if remainder != 0 {
            let padding_size = WORD_SIZE_BYTES - remainder;
            self.data.resize(self.data.len() + padding_size, 0);
        }
    }
}

impl Debug for ResolvedDataSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedDataSegment")
            .field("offset", &self.offset)
            .field("size", &self.data.len())
            .field("readonly", &self.readonly)
            .field("data", &self.data.to_hex())
            .finish()
    }
}

/// Returns an error if any two segments overlap
fn validate_no_overlaps(segments: &[ResolvedDataSegment]) -> Result<(), Report> {
    assert!(segments.is_sorted_by_key(|s| s.offset), "Segments must be sorted by offset");
    for window in segments.windows(2) {
        let current = &window[0];
        let next = &window[1];
        if current.end_offset() > next.offset {
            let error_msg = format!(
                "Data segments overlap: segment at offset {:#x} (size {}) overlaps with segment \
                 at offset {:#x}",
                current.offset,
                current.data.len(),
                next.offset
            );
            return Err(Report::msg(error_msg));
        }
    }
    Ok(())
}

/// Merges segment metadata (names and readonly status)
/// Returns a  whether all segments are readonly
fn merge_metadata(segments: &[ResolvedDataSegment]) -> bool {
    let mut all_readonly = true;

    for segment in segments.iter() {
        all_readonly = all_readonly && segment.readonly;
    }

    all_readonly
}

/// Copies all segment data into the merged buffer at their respective offsets
fn copy_segments_to_buffer(segments: &[ResolvedDataSegment], buffer: &mut [u8], base_offset: u32) {
    for segment in segments {
        let relative_offset = (segment.offset - base_offset) as usize;
        let end = relative_offset + segment.data.len();
        buffer[relative_offset..end].copy_from_slice(&segment.data);
    }
}

/// Merge all data segments into a single segment and pad to 16-byte alignment
///
/// Requirements:
/// - Segments cannot have their offsets moved
/// - Segments must not overlap (returns error if they do)
/// - All segments are merged into one
/// - The resulting data is padded with zeros to be divisible by 16
///
/// Returns `None` if the input segments are empty, otherwise returns the merged segment.
pub fn merge_data_segments(
    mut segments: SmallVec<[ResolvedDataSegment; 2]>,
) -> Result<Option<ResolvedDataSegment>, Report> {
    if segments.is_empty() {
        return Ok(None);
    }

    // Early return for single segment - just pad it
    if segments.len() == 1 {
        let mut segment = segments.pop().unwrap();
        segment.pad_to_word_alignment();
        return Ok(Some(segment));
    }

    segments.sort_by_key(|s| s.offset);

    validate_no_overlaps(&segments)?;

    let base_offset = segments[0].offset;
    let last_segment = segments.last().unwrap();
    let initial_size = (last_segment.end_offset() - base_offset) as usize;

    let mut merged_data = vec![0u8; initial_size];

    copy_segments_to_buffer(&segments, &mut merged_data, base_offset);

    let all_readonly = merge_metadata(&segments);

    let mut merged_segment = ResolvedDataSegment {
        offset: base_offset,
        data: merged_data,
        readonly: all_readonly,
    };

    merged_segment.pad_to_word_alignment();

    Ok(Some(merged_segment))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_segments() {
        let segments = SmallVec::new();
        let merged = merge_data_segments(segments).unwrap();
        assert!(merged.is_none());
    }

    #[test]
    fn test_single_segment_with_padding() {
        let mut segments = SmallVec::new();
        segments.push(ResolvedDataSegment {
            offset: 10,
            data: vec![1, 2, 3],
            readonly: true,
        });

        let merged = merge_data_segments(segments).unwrap().unwrap();

        assert_eq!(merged.offset, 10);
        // 3 bytes padded to 16
        assert_eq!(merged.data.len(), 16);
        assert_eq!(&merged.data[0..3], &[1, 2, 3]);
        assert_eq!(&merged.data[3..], &[0; 13]);
    }

    #[test]
    fn test_padding_bytes_are_zeros() {
        // Test various data sizes to verify padding is always zeros
        let test_cases = vec![
            (vec![0x42], 1usize, 15),  // 1 byte data, 15 bytes padding
            (vec![0xaa, 0xbb], 2, 14), // 2 bytes data, 14 bytes padding
            (vec![0x11; 5], 5, 11),    // 5 bytes data, 11 bytes padding
            (vec![0xff; 13], 13, 3),   // 13 bytes data, 3 bytes padding
            (vec![0x77; 15], 15, 1),   // 15 bytes data, 1 byte padding
            (vec![0x88; 16], 16, 0),   // 16 bytes data, no padding
            (vec![0x99; 17], 17, 15),  // 17 bytes data, 15 bytes padding
            (vec![0xcc; 31], 31, 1),   // 31 bytes data, 1 byte padding
        ];

        for (data, data_len, expected_padding) in test_cases {
            let mut segments = SmallVec::new();
            segments.push(ResolvedDataSegment {
                offset: 1000,
                data: data.clone(),
                readonly: false,
            });

            let merged = merge_data_segments(segments).unwrap().unwrap();

            let result_data = &merged.data;
            let expected_total_len = data_len.next_multiple_of(16); // Round up to nearest 16
            assert_eq!(result_data.len(), expected_total_len);

            // Verify original data is preserved
            assert_eq!(&result_data[0..data_len], &data[..]);

            // Verify all padding bytes are zeros
            if expected_padding > 0 {
                let padding_slice = &result_data[data_len..];
                assert_eq!(padding_slice.len(), expected_padding);
                assert!(
                    padding_slice.iter().all(|&b| b == 0),
                    "Padding bytes should all be zero for {data_len} bytes of data"
                );
            }
        }
    }

    #[test]
    fn test_data_accessible_at_original_offsets() {
        // Test that after merging, data can be accessed at original offsets
        let mut segments = SmallVec::new();

        // Segment 1 at offset 100
        segments.push(ResolvedDataSegment {
            offset: 100,
            data: vec![0xaa, 0xbb, 0xcc, 0xdd],
            readonly: false,
        });

        // Segment 2 at offset 110 (gap of 6 bytes)
        segments.push(ResolvedDataSegment {
            offset: 110,
            data: vec![0x11, 0x22],
            readonly: false,
        });

        // Segment 3 at offset 120
        segments.push(ResolvedDataSegment {
            offset: 120,
            data: vec![0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa],
            readonly: false,
        });

        let merged = merge_data_segments(segments).unwrap().unwrap();

        assert_eq!(merged.offset, 100);

        // Verify each segment's data is at the correct position
        let data = &merged.data;

        // Data from segment 1 (offset 100-103)
        assert_eq!(&data[0..4], &[0xaa, 0xbb, 0xcc, 0xdd]);

        // Gap (offset 104-109) should be zeros
        assert_eq!(&data[4..10], &[0, 0, 0, 0, 0, 0]);

        // Data from segment 2 (offset 110-111)
        assert_eq!(&data[10..12], &[0x11, 0x22]);

        // Gap (offset 112-119) should be zeros
        assert_eq!(&data[12..20], &[0, 0, 0, 0, 0, 0, 0, 0]);

        // Data from segment 3 (offset 120-125)
        assert_eq!(&data[20..26], &[0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa]);

        // Total size should be padded to multiple of 16
        // Range is 100-126 (26 bytes), padded to 32
        assert_eq!(data.len(), 32);

        // Padding should be zeros
        assert_eq!(&data[26..], &[0; 6]);
    }

    #[test]
    fn test_merge_segments_with_gap() {
        let mut segments = SmallVec::new();
        segments.push(ResolvedDataSegment {
            offset: 0,
            data: vec![1, 2, 3, 4],
            readonly: true,
        });
        segments.push(ResolvedDataSegment {
            offset: 8,
            data: vec![5, 6, 7, 8],
            readonly: true,
        });

        let merged = merge_data_segments(segments).unwrap().unwrap();

        assert_eq!(merged.offset, 0);
        // 4 bytes + 4 gap + 4 bytes = 12, padded to 16
        assert_eq!(merged.data.len(), 16);
        assert_eq!(merged.data, vec![1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0]);
    }

    #[test]
    fn test_overlapping_segments_error() {
        let mut segments = SmallVec::new();
        segments.push(ResolvedDataSegment {
            offset: 0,
            data: vec![1, 2, 3, 4, 5, 6],
            readonly: true,
        });
        segments.push(ResolvedDataSegment {
            offset: 4,
            data: vec![7, 8],
            readonly: false,
        });

        // This should return an error because segments overlap
        let result = merge_data_segments(segments);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = format!("{err}");
        assert!(err_msg.contains("Data segments overlap"));
    }

    #[test]
    fn test_p2id_case() {
        // Simulate the p2id case with .rodata and .data segments
        let mut segments = SmallVec::new();
        segments.push(ResolvedDataSegment {
            offset: 1048576,   // 0x100000
            data: vec![0; 44], // Size of the .rodata string
            readonly: true,
        });
        segments.push(ResolvedDataSegment {
            offset: 1048620,   // 0x10002C
            data: vec![0; 76], // Size of the .data segment
            readonly: false,
        });

        let merged = merge_data_segments(segments).unwrap().unwrap();

        assert_eq!(merged.offset, 1048576);
        // .rodata (44) + gap (0) + .data (76) = 120, padded to 128
        assert_eq!(merged.data.len(), 128);
        assert!(!merged.readonly);
    }

    #[test]
    fn test_already_aligned_size() {
        let mut segments = SmallVec::new();
        segments.push(ResolvedDataSegment {
            offset: 100,
            data: vec![1; 32], // Already divisible by 16
            readonly: true,
        });

        let merged = merge_data_segments(segments).unwrap().unwrap();

        assert_eq!(merged.offset, 100);
        assert_eq!(merged.data.len(), 32); // No padding needed
    }
}
