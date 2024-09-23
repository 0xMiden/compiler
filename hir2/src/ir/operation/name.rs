use core::fmt;

use crate::{interner, DialectName};

/// The operation name, or mnemonic, that uniquely identifies an operation.
///
/// The operation name consists of its dialect name, and the opcode name within the dialect.
///
/// No two operation names can share the same fully-qualified operation name.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationName {
    /// The dialect of this operation
    pub dialect: DialectName,
    /// The opcode name for this operation
    pub name: interner::Symbol,
}
impl OperationName {
    pub fn new<S>(dialect: DialectName, name: S) -> Self
    where
        S: Into<interner::Symbol>,
    {
        Self {
            dialect,
            name: name.into(),
        }
    }
}
impl fmt::Debug for OperationName {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for OperationName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", &self.dialect, &self.name)
    }
}
