use alloc::vec::Vec;

use crate::{OperationRef, Report};

/// A recorded reason that a conversion pattern did not match.
pub struct MatchFailure {
    op: OperationRef,
    reason: Report,
}

impl MatchFailure {
    #[inline]
    pub const fn op(&self) -> OperationRef {
        self.op
    }

    #[inline]
    pub const fn reason(&self) -> &Report {
        &self.reason
    }
}

/// Rewriter used by conversion patterns.
///
/// The Phase 4 driver will extend this with a listener-backed wrapper around
/// the existing pattern rewriter.
pub struct ConversionPatternRewriter {
    match_failures: Vec<MatchFailure>,
}

impl ConversionPatternRewriter {
    #[inline]
    pub const fn new() -> Self {
        Self {
            match_failures: Vec::new(),
        }
    }

    pub fn notify_match_failure(&mut self, op: OperationRef, reason: Report) {
        self.match_failures.push(MatchFailure { op, reason });
    }

    #[inline]
    pub fn match_failures(&self) -> &[MatchFailure] {
        &self.match_failures
    }

    #[inline]
    pub fn clear_match_failures(&mut self) {
        self.match_failures.clear();
    }
}

impl Default for ConversionPatternRewriter {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use crate::{
        Context, OpRegistration, Report, conversion::ConversionPatternRewriter,
        dialects::test::Constant,
    };

    #[test]
    fn records_match_failures() {
        let context = Rc::new(Context::default());
        let op = context
            .get_or_register_dialect::<<Constant as OpRegistration>::Dialect>()
            .expect_registered_name::<Constant>()
            .alloc_default(context);
        let mut rewriter = ConversionPatternRewriter::new();

        rewriter.notify_match_failure(op, Report::msg("no match"));

        assert_eq!(rewriter.match_failures().len(), 1);
        rewriter.clear_match_failures();
        assert!(rewriter.match_failures().is_empty());
    }
}
