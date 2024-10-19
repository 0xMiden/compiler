use core::{fmt, str::FromStr};

use crate::define_attr_type;

/// The types of visibility that a [Symbol] may have
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// Internal visibility implies that we know all uses of the symbol, but that there may be uses
    /// in other symbol tables in addition to the current one.
    Internal,
}
define_attr_type!(Visibility);
impl Visibility {
    #[inline]
    pub fn is_public(&self) -> bool {
        matches!(self, Self::Public)
    }

    #[inline]
    pub fn is_private(&self) -> bool {
        matches!(self, Self::Private)
    }

    #[inline]
    pub fn is_internal(&self) -> bool {
        matches!(self, Self::Internal)
    }
}
impl crate::formatter::PrettyPrint for Visibility {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        match self {
            Self::Public => const_text("public"),
            Self::Private => const_text("private"),
            Self::Internal => const_text("internal"),
        }
    }
}
impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => f.write_str("public"),
            Self::Private => f.write_str("private"),
            Self::Internal => f.write_str("internal"),
        }
    }
}
impl FromStr for Visibility {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" => Ok(Self::Public),
            "private" => Ok(Self::Private),
            "internal" => Ok(Self::Internal),
            _ => Err(()),
        }
    }
}
