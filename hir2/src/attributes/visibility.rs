use core::{fmt, str::FromStr};

/// The types of visibility that a [Symbol] may have
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    /// The symbol is public and may be referenced anywhere internal or external to the visible
    /// references in the IR.
    ///
    /// Public visibility implies that we cannot remove the symbol even if we are unaware of any
    /// references, and no other constraints apply, as we must assume that the symbol has references
    /// we don't know about.
    #[default]
    Public,
    /// The symbol is private and may only be referenced by ops local to operations within the
    /// current symbol table.
    ///
    /// Private visibility implies that we know all uses of the symbol, and that those uses must
    /// all exist within the current symbol table.
    Private,
    /// The symbol is public, but may only be referenced by symbol tables in the current compilation
    /// graph, thus retaining the ability to observe all uses, and optimize based on that
    /// information.
    ///
    /// Nested visibility implies that we know all uses of the symbol, but that there may be uses
    /// in other symbol tables in addition to the current one.
    Nested,
}
impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => f.write_str("public"),
            Self::Private => f.write_str("private"),
            Self::Nested => f.write_str("nested"),
        }
    }
}
impl FromStr for Visibility {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" => Ok(Self::Public),
            "private" => Ok(Self::Private),
            "nested" => Ok(Self::Nested),
            _ => Err(()),
        }
    }
}
