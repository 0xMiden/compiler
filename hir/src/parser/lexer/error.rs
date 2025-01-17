use core::{fmt, num::IntErrorKind};

use crate::diagnostics::{miette, Diagnostic, SourceSpan};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum InvalidEscapeKind {
    Empty,
    InvalidChars,
    Invalid,
}
impl fmt::Display for InvalidEscapeKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("cannot be empty"),
            Self::InvalidChars => f.write_str("contained one or more invalid characters"),
            Self::Invalid => f.write_str("is not recognized as a valid escape"),
        }
    }
}

/// Errors that may occur during lexing of the source
#[derive(Clone, Debug, thiserror::Error, Diagnostic)]
pub enum LexicalError {
    #[error("invalid integer value: {}", DisplayIntErrorKind(reason))]
    #[diagnostic()]
    InvalidInt {
        #[label]
        span: SourceSpan,
        reason: IntErrorKind,
    },
    #[error("encountered unexpected character '{found}'")]
    #[diagnostic()]
    UnexpectedCharacter {
        #[label]
        start: SourceSpan,
        found: char,
    },
    #[error("unclosed string")]
    #[diagnostic()]
    UnclosedString {
        #[label]
        span: SourceSpan,
    },
    #[error("invalid unicode escape: {kind}")]
    #[diagnostic()]
    InvalidUnicodeEscape {
        #[label]
        span: SourceSpan,
        kind: InvalidEscapeKind,
    },
    #[error("invalid hex escape: {kind}")]
    #[diagnostic()]
    InvalidHexEscape {
        #[label]
        span: SourceSpan,
        kind: InvalidEscapeKind,
    },
    #[error("invalid module identifier")]
    #[diagnostic(help(
        "module names must be non-empty, start with 'a-z', and only contain ascii alpha-numeric \
         characters, '_', or '::' as a namespacing operator",
    ))]
    InvalidModuleIdentifier {
        #[label]
        span: SourceSpan,
    },
    #[error("invalid function identifier")]
    #[diagnostic(help("function names must be non-empty, and start with '_' or 'a-z'"))]
    InvalidFunctionIdentifier {
        #[label]
        span: SourceSpan,
    },
}
impl PartialEq for LexicalError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::InvalidInt { reason: lhs, .. }, Self::InvalidInt { reason: rhs, .. }) => {
                lhs == rhs
            }
            (
                Self::UnexpectedCharacter { found: lhs, .. },
                Self::UnexpectedCharacter { found: rhs, .. },
            ) => lhs == rhs,
            (Self::UnclosedString { .. }, Self::UnclosedString { .. }) => true,
            (
                Self::InvalidUnicodeEscape { kind: k1, .. },
                Self::InvalidUnicodeEscape { kind: k2, .. },
            ) => k1 == k2,
            (Self::InvalidHexEscape { kind: k1, .. }, Self::InvalidHexEscape { kind: k2, .. }) => {
                k1 == k2
            }
            (Self::InvalidModuleIdentifier { .. }, Self::InvalidModuleIdentifier { .. }) => true,
            (Self::InvalidFunctionIdentifier { .. }, Self::InvalidFunctionIdentifier { .. }) => {
                true
            }
            _ => false,
        }
    }
}

struct DisplayIntErrorKind<'a>(&'a IntErrorKind);
impl fmt::Display for DisplayIntErrorKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            IntErrorKind::Empty => write!(f, "unable to parse empty string as integer"),
            IntErrorKind::InvalidDigit => write!(f, "invalid digit"),
            IntErrorKind::PosOverflow => write!(f, "value is too big"),
            IntErrorKind::NegOverflow => write!(f, "value is too big"),
            IntErrorKind::Zero => write!(f, "zero is not a valid value here"),
            other => write!(f, "unable to parse integer value: {:?}", other),
        }
    }
}
