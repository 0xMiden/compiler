//! Error types for note storage schemas.

use core::fmt;

/// An error reported while reading, encoding, or decoding a note storage schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    message: String,
}

impl Error {
    /// Creates an error with an actionable message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Adds context before the current error message.
    pub(crate) fn context(self, context: impl fmt::Display) -> Self {
        Self::new(format!("{context}: {}", self.message))
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
