use core::{fmt, str::FromStr};

use crate::{
    AttrPrinter, SmallVec, attributes::AttrParser, derive::DialectAttribute,
    dialects::builtin::BuiltinDialect, print::AsmPrinter,
};

/// The types of visibility that a [Symbol] may have
#[derive(DialectAttribute, Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
#[repr(u8)]
pub enum Visibility {
    /// The symbol is public and may be referenced anywhere internal or external to the visible
    /// references in the IR.
    ///
    /// Public visibility implies that we cannot remove the symbol even if we are unaware of any
    /// references, and no other constraints apply, as we must assume that the symbol has references
    /// we don't know about.
    Public,
    /// The symbol is private and may only be referenced by ops local to operations within the
    /// current symbol table.
    ///
    /// Private visibility implies that we know all uses of the symbol, and that those uses must
    /// all exist within the current symbol table.
    #[default]
    Private,
    /// The symbol is public, but may only be referenced by symbol tables in the current compilation
    /// graph, thus retaining the ability to observe all uses, and optimize based on that
    /// information.
    ///
    /// Internal visibility implies that we know all uses of the symbol, but that there may be uses
    /// in other symbol tables in addition to the current one.
    Internal,
}

impl AttrPrinter for VisibilityAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_keyword(self.value.as_str());
    }
}

impl AttrParser for Visibility {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        use crate::parse::Token;

        let keywords = SmallVec::<[Token; 4]>::from_iter(
            ([Visibility::Public, Visibility::Private, Visibility::Internal])
                .iter()
                .map(Visibility::as_str)
                .map(Token::BareIdent),
        );

        let visibility = parser.parse_keyword_from(&keywords)?;
        let visibility = visibility.as_str().parse::<Visibility>().unwrap();

        let attr = parser.context_rc().create_attribute::<VisibilityAttr, _>(visibility);
        Ok(attr)
    }
}

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

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Internal => "internal",
        }
    }
}

impl AsRef<str> for Visibility {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
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
        f.write_str(self.as_ref())
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
