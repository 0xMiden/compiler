use super::{ConversionPatternRewriter, TypeConverter};
use crate::{
    Op, OperationRef, Report, SmallVec, UnsafeIntrusiveEntityRef, ValueRef, patterns::Pattern,
};

/// Operands remapped through the current conversion value mapping.
pub struct ConvertedOperands<'a> {
    groups: &'a [SmallVec<[ValueRef; 2]>],
}

impl<'a> ConvertedOperands<'a> {
    #[inline]
    pub const fn new(groups: &'a [SmallVec<[ValueRef; 2]>]) -> Self {
        Self { groups }
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.groups.len()
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&'a [ValueRef]> {
        self.groups.get(index).map(|group| group.as_slice())
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &'a [ValueRef]> {
        self.groups.iter().map(|group| group.as_slice())
    }
}

/// A rewrite pattern that participates in target-driven dialect conversion.
pub trait ConversionPattern: Pattern {
    fn type_converter(&self) -> Option<&TypeConverter> {
        None
    }

    fn match_and_rewrite(
        &self,
        op: OperationRef,
        operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report>;
}

/// Typed adapter trait for conversion patterns rooted on a concrete operation.
pub trait OpConversionPattern<T: Op>: ConversionPattern {
    fn match_and_rewrite_typed(
        &self,
        op: UnsafeIntrusiveEntityRef<T>,
        operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report>;
}
