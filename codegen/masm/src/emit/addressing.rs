use miden_core::Felt;
use midenc_hir::SourceSpan;
use smallvec::{SmallVec, smallvec};

use super::masm::{self, Instruction as I};

/// Instructions implementing one byte-address stack transition.
pub(crate) type AddressSequence = SmallVec<[I; 8]>;

/// The byte-address domain used consistently by a lowering unit and its nested emitters.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ByteAddressing {
    /// Legacy byte pointers and their arithmetic are u32 values.
    #[default]
    Narrow,
    /// Native memory contains 2^32 elements, addressed by 2^34 bytes.
    Wide,
}

impl ByteAddressing {
    pub(crate) const LIMIT: u64 = 1 << 34;

    /// `[byte] -> [element, offset]`. Native memory operations reject element addresses >=2^32.
    pub(crate) fn native_ptr(self) -> AddressSequence {
        match self {
            Self::Narrow => smallvec![I::U32DivModImm(4.into()), I::Swap1],
            // offset = lo % 4; element = hi * 2^30 + lo / 4.
            Self::Wide => smallvec![
                I::U32Split,
                I::U32DivModImm(4.into()),
                I::MovDn2,
                I::Swap1,
                I::MulImm(Felt::from(1u32 << 30).into()),
                I::Add,
            ],
        }
    }

    /// `[byte] -> [element]`, asserting the requested alignment through `unaligned`.
    pub(crate) fn aligned_element_addr(self, alignment: u32, unaligned: I) -> AddressSequence {
        let factor = alignment / 4;
        match self {
            Self::Narrow => {
                let mut ops = smallvec![I::U32DivModImm(alignment.into()), unaligned];
                if factor != 1 {
                    ops.push(I::U32WrappingMulImm(factor.into()));
                }
                ops
            }
            Self::Wide => {
                let mut ops = smallvec![I::U32Split, I::U32DivModImm(alignment.into()), unaligned];
                if factor != 1 {
                    ops.push(I::MulImm(Felt::from(factor).into()));
                }
                ops.extend([I::Swap1, I::MulImm(Felt::from(1u32 << 30).into()), I::Add]);
                ops
            }
        }
    }

    /// `[src,dst,count] -> [count/4,src/4,dst/4]`, checking all three alignments.
    pub(crate) fn element_copy_operands(self, errors: [I; 3]) -> AddressSequence {
        let [src, dst, count] = errors;
        let mut ops = self.aligned_element_addr(4, src);
        ops.push(I::Swap1);
        ops.extend(self.aligned_element_addr(4, dst));
        ops.extend([I::Swap2, I::U32DivModImm(4.into()), count]);
        ops
    }

    /// `[byte] -> [lo32]`; alignment depends only on this limb.
    pub(crate) fn low_limb(self) -> AddressSequence {
        match self {
            Self::Narrow => smallvec![],
            Self::Wide => smallvec![I::U32Split, I::Swap1, I::Drop],
        }
    }

    /// Whether src, dst and byte count are all element aligned; preserves the operands.
    pub(crate) fn element_copy_alignment(self) -> AddressSequence {
        let mut ops = AddressSequence::new();
        for (index, (dup, address)) in
            [(I::Dup0, true), (I::Dup2, true), (I::Dup3, false)].into_iter().enumerate()
        {
            ops.push(dup);
            if address {
                ops.extend(self.low_limb());
            }
            ops.extend([I::U32DivModImm(4.into()), I::Swap1, I::Drop, I::EqImm(Felt::ZERO.into())]);
            if index != 0 {
                ops.push(I::And);
            }
        }
        ops
    }

    pub(crate) fn add(self) -> I {
        match self {
            Self::Narrow => I::U32WrappingAdd,
            Self::Wide => I::Add,
        }
    }

    pub(crate) fn lte(self) -> I {
        match self {
            Self::Narrow => I::U32Lte,
            Self::Wide => I::Lte,
        }
    }

    pub(crate) fn gt(self) -> I {
        match self {
            Self::Narrow => I::U32Gt,
            Self::Wide => I::Gt,
        }
    }

    /// `[i,base] -> [base + i*size]`; the enclosing range check bounds the result.
    pub(crate) fn offset_by_index(self, size: u32, span: SourceSpan) -> AddressSequence {
        match (self, size) {
            (Self::Narrow, 1) => smallvec![I::U32WrappingAdd],
            (Self::Narrow, _) => smallvec![
                I::Push(masm::Immediate::Value(masm::Span::new(span, size.into()))),
                I::U32WrappingMadd,
            ],
            (Self::Wide, 1) => smallvec![I::Add],
            (Self::Wide, _) => smallvec![I::MulImm(Felt::from(size).into()), I::Add],
        }
    }

    /// `[start] -> [start]`, rejecting invalid pointers before any field addition can wrap.
    pub(crate) fn validate_start(self, error: I) -> AddressSequence {
        match self {
            Self::Narrow => smallvec![],
            Self::Wide => smallvec![
                I::Dup0,
                I::Push(masm::PushValue::from(Felt::new_unchecked(Self::LIMIT)).into()),
                I::Gte,
                error,
            ],
        }
    }

    /// `[length,start] -> [end]`. Wide endpoints may equal the exclusive native limit.
    pub(crate) fn range_end(self, error: I) -> AddressSequence {
        match self {
            Self::Narrow => smallvec![I::U32OverflowingAdd, error],
            Self::Wide => {
                let mut ops = smallvec![I::Swap1];
                ops.extend(self.validate_start(error.clone()));
                ops.extend([
                    I::Add,
                    I::Dup0,
                    I::Push(masm::PushValue::from(Felt::new_unchecked(Self::LIMIT)).into()),
                    I::Gt,
                    error,
                ]);
                ops
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_byte_addressing_preserves_legacy_sequences() {
        let mode = ByteAddressing::default();
        assert_eq!(mode, ByteAddressing::Narrow);
        assert_eq!(mode.native_ptr().as_slice(), &[I::U32DivModImm(4.into()), I::Swap1]);
        assert_eq!(
            mode.aligned_element_addr(16, I::Assertz).as_slice(),
            &[I::U32DivModImm(16.into()), I::Assertz, I::U32WrappingMulImm(4.into())],
        );
        assert_eq!(
            mode.element_copy_operands([I::Assertz, I::Assertz, I::Assertz]).as_slice(),
            &[
                I::U32DivModImm(4.into()),
                I::Assertz,
                I::Swap1,
                I::U32DivModImm(4.into()),
                I::Assertz,
                I::Swap2,
                I::U32DivModImm(4.into()),
                I::Assertz,
            ],
        );
        assert_eq!(mode.range_end(I::Assertz).as_slice(), &[I::U32OverflowingAdd, I::Assertz]);
        assert!(mode.low_limb().is_empty());
        assert!(mode.validate_start(I::Assertz).is_empty());
        assert_eq!(mode.add(), I::U32WrappingAdd);
        assert_eq!(mode.lte(), I::U32Lte);
        assert_eq!(mode.gt(), I::U32Gt);
    }
}
