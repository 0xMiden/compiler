use core::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    str::FromStr,
};

use anyhow::anyhow;

use super::{
    interner::{symbols, Symbol},
    SourceSpan, Spanned,
};
use crate::{
    define_attr_type,
    formatter::{self, PrettyPrint},
};

/// Represents a globally-unique module/function name pair, with corresponding source spans.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Spanned)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FunctionIdent {
    pub module: Ident,
    #[span]
    pub function: Ident,
}
define_attr_type!(FunctionIdent);
impl FunctionIdent {
    pub fn display(&self) -> impl fmt::Display + '_ {
        use crate::formatter::*;

        flatten(
            const_text(self.module.as_str())
                + const_text("::")
                + const_text(self.function.as_str()),
        )
    }
}
impl FromStr for FunctionIdent {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.rsplit_once("::") {
            Some((ns, id)) => {
                let module = Ident::with_empty_span(Symbol::intern(ns));
                let function = Ident::with_empty_span(Symbol::intern(id));
                Ok(Self { module, function })
            }
            None => Err(anyhow!(
                "invalid function name, expected fully-qualified identifier, e.g. \
                 'std::math::u64::checked_add'"
            )),
        }
    }
}
impl fmt::Debug for FunctionIdent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FunctionIdent")
            .field("module", &self.module.name)
            .field("function", &self.function.name)
            .finish()
    }
}
impl fmt::Display for FunctionIdent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}
impl PrettyPrint for FunctionIdent {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        flatten(const_text("(") + display(self.module) + const_text(" ") + display(self.function))
    }
}
impl PartialOrd for FunctionIdent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FunctionIdent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.module.cmp(&other.module).then(self.function.cmp(&other.function))
    }
}

/// Represents an identifier in the IR.
///
/// An identifier is some string, along with an associated source span
#[derive(Copy, Clone, Eq, Spanned)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "Symbol", from = "Symbol"))]
pub struct Ident {
    pub name: Symbol,
    #[span]
    pub span: SourceSpan,
}
define_attr_type!(Ident);
impl Default for Ident {
    fn default() -> Self {
        Self {
            name: symbols::Empty,
            span: SourceSpan::UNKNOWN,
        }
    }
}
impl FromStr for Ident {
    type Err = core::convert::Infallible;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(name))
    }
}
impl<'a> From<&'a str> for Ident {
    fn from(name: &'a str) -> Self {
        Self::with_empty_span(Symbol::intern(name))
    }
}
impl From<Symbol> for Ident {
    #[inline]
    fn from(sym: Symbol) -> Self {
        Self::with_empty_span(sym)
    }
}
impl From<Ident> for Symbol {
    #[inline]
    fn from(id: Ident) -> Self {
        id.as_symbol()
    }
}
impl Ident {
    #[inline]
    pub const fn new(name: Symbol, span: SourceSpan) -> Ident {
        Ident { name, span }
    }

    #[inline]
    pub const fn with_empty_span(name: Symbol) -> Ident {
        Ident::new(name, SourceSpan::UNKNOWN)
    }

    #[inline]
    pub fn as_str(self) -> &'static str {
        self.name.as_str()
    }

    #[inline(always)]
    pub fn as_symbol(self) -> Symbol {
        self.name
    }

    // An identifier can be unquoted if is composed of any sequence of printable
    // ASCII characters, except whitespace, quotation marks, comma, semicolon, or brackets
    pub fn requires_quoting(&self) -> bool {
        self.as_str().contains(|c| match c {
            c if c.is_ascii_control() => true,
            ' ' | '\'' | '"' | ',' | ';' | '[' | ']' => true,
            c if c.is_ascii_graphic() => false,
            _ => true,
        })
    }
}
impl AsRef<str> for Ident {
    #[inline(always)]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl alloc::borrow::Borrow<Symbol> for Ident {
    #[inline]
    fn borrow(&self) -> &Symbol {
        &self.name
    }
}
impl alloc::borrow::Borrow<str> for Ident {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
impl Ord for Ident {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}
impl PartialOrd for Ident {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Ident {
    #[inline]
    fn eq(&self, rhs: &Self) -> bool {
        self.name == rhs.name
    }
}
impl PartialEq<Symbol> for Ident {
    #[inline]
    fn eq(&self, rhs: &Symbol) -> bool {
        self.name.eq(rhs)
    }
}
impl PartialEq<str> for Ident {
    fn eq(&self, rhs: &str) -> bool {
        self.name.as_str() == rhs
    }
}
impl Hash for Ident {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}
impl fmt::Debug for Ident {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}
impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}
impl PrettyPrint for Ident {
    fn render(&self) -> formatter::Document {
        use crate::formatter::*;

        if self.requires_quoting() {
            text(format!("\"{}\"", self.as_str().escape_default()))
        } else {
            text(format!("#{}", self.as_str()))
        }
    }
}
