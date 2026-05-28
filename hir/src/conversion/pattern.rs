use super::{ConversionPatternRewriter, TypeConverter};
use crate::{
    Op, OperationRef, Report, SmallVec, UnsafeIntrusiveEntityRef, ValueRef, patterns::Pattern,
};

/// Operands remapped through the current conversion value mapping.
///
/// Each source operand maps to a group of converted values. The initial driver supports 1:1 type
/// conversion for boundary materialization, but this view is shaped for future 1:N conversions so
/// pattern code does not need another API break when that support is added.
pub struct ConvertedOperands<'a> {
    groups: &'a [SmallVec<[ValueRef; 2]>],
}

impl<'a> ConvertedOperands<'a> {
    /// Create a converted operand view over precomputed operand groups.
    #[inline]
    pub const fn new(groups: &'a [SmallVec<[ValueRef; 2]>]) -> Self {
        Self { groups }
    }

    /// Return true when the root operation has no operands.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Return the number of source operands represented by this view.
    #[inline]
    pub const fn len(&self) -> usize {
        self.groups.len()
    }

    /// Return the converted value group for source operand `index`.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&'a [ValueRef]> {
        self.groups.get(index).map(|group| group.as_slice())
    }

    /// Iterate over converted value groups in source operand order.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &'a [ValueRef]> {
        self.groups.iter().map(|group| group.as_slice())
    }
}

/// A rewrite pattern that participates in target-driven dialect conversion.
pub trait ConversionPattern: Pattern {
    /// Return the type converter used to remap operands before this pattern runs.
    ///
    /// Patterns that rewrite type-changing operations should return the same converter they use
    /// when building replacement IR. Patterns that do not need type remapping can use the default
    /// `None`, which passes original operands through unchanged.
    fn type_converter(&self) -> Option<&TypeConverter> {
        None
    }

    /// Try to rewrite `op` using converted operands and the conversion rewriter.
    ///
    /// Return `Ok(true)` after successfully mutating IR. Return `Ok(false)` when the pattern does
    /// not match; in that case the pattern must not mutate IR, but may call
    /// [`ConversionPatternRewriter::notify_match_failure`] to leave a diagnostic breadcrumb. Return
    /// `Err` for a fatal failure that should abort conversion. The full-conversion driver checks
    /// this mutation contract because it does not provide rollback.
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report>;
}

/// Typed adapter trait for conversion patterns rooted on a concrete operation.
pub trait OpConversionPattern<T: Op>: ConversionPattern {
    /// Try to rewrite a statically typed root operation.
    ///
    /// This has the same mutation and result contract as
    /// [`ConversionPattern::match_and_rewrite`], but lets pattern implementations work with their
    /// concrete operation type after the caller has performed the root-type check.
    fn match_and_rewrite_typed(
        &self,
        op: UnsafeIntrusiveEntityRef<T>,
        operands: ConvertedOperands<'_>,
        rewriter: &mut ConversionPatternRewriter,
    ) -> Result<bool, Report>;
}
