//! Error types for note storage schemas.

use core::fmt;

/// Why a bundled codec call did not return a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecFailure {
    /// The call used its whole fuel budget.
    OutOfFuel,
    /// The call hit a memory or table limit.
    LimitExceeded,
    /// The component trapped, or the engine rejected the call.
    Trapped,
    /// The codec returned its own rejection message.
    Rejected,
}

/// An error reported while reading, encoding, or decoding a note storage schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    message: String,
    codec_failure: Option<CodecFailure>,
}

impl Error {
    /// Creates an error with an actionable message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            codec_failure: None,
        }
    }

    /// Creates an error that reports how a bundled codec call failed.
    ///
    /// Only the bundled codec adapter reports a failure class.
    #[cfg(feature = "codec-component")]
    pub(crate) fn codec(kind: CodecFailure, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            codec_failure: Some(kind),
        }
    }

    /// Returns the failure class of a bundled codec call.
    ///
    /// Errors from other sources return `None`.
    pub fn codec_failure(&self) -> Option<CodecFailure> {
        self.codec_failure
    }

    /// Adds context before the current error message.
    pub(crate) fn context(self, context: impl fmt::Display) -> Self {
        Self {
            message: format!("{context}: {}", self.message),
            codec_failure: self.codec_failure,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// A result returned by note storage schema operations.
pub type Result<T> = core::result::Result<T, Error>;
