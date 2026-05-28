use crate::Report;

/// The result of evaluating a dynamic legality predicate.
pub enum DynamicLegalityResult {
    /// The operation instance is legal for the target.
    Legal,
    /// The operation instance is illegal for the target.
    ///
    /// `reason`, when present, is reported to callers as the explanation for the dynamic legality
    /// failure.
    Illegal { reason: Option<Report> },
}

impl DynamicLegalityResult {
    /// Return a legal dynamic legality result.
    #[inline]
    pub const fn legal() -> Self {
        Self::Legal
    }

    /// Return an illegal dynamic legality result without an explanatory diagnostic.
    #[inline]
    pub const fn illegal() -> Self {
        Self::Illegal { reason: None }
    }

    /// Return an illegal dynamic legality result with an explanatory diagnostic.
    #[inline]
    pub fn illegal_with_reason(reason: Report) -> Self {
        Self::Illegal {
            reason: Some(reason),
        }
    }

    /// Return `Legal` when `condition` is true, otherwise return an illegal result.
    #[inline]
    pub const fn legal_if(condition: bool) -> Self {
        if condition {
            Self::Legal
        } else {
            Self::Illegal { reason: None }
        }
    }

    /// Return true when this result is legal.
    #[inline]
    pub const fn is_legal(&self) -> bool {
        matches!(self, Self::Legal)
    }
}

impl From<bool> for DynamicLegalityResult {
    #[inline]
    fn from(condition: bool) -> Self {
        Self::legal_if(condition)
    }
}
