use crate::Report;

/// The result of evaluating a dynamic legality predicate.
pub enum DynamicLegalityResult {
    Legal,
    Illegal { reason: Option<Report> },
}

impl DynamicLegalityResult {
    #[inline]
    pub const fn legal() -> Self {
        Self::Legal
    }

    #[inline]
    pub const fn illegal() -> Self {
        Self::Illegal { reason: None }
    }

    #[inline]
    pub fn illegal_with_reason(reason: Report) -> Self {
        Self::Illegal {
            reason: Some(reason),
        }
    }

    #[inline]
    pub const fn legal_if(condition: bool) -> Self {
        if condition {
            Self::Legal
        } else {
            Self::Illegal { reason: None }
        }
    }

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
