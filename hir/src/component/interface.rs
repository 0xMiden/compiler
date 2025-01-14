use core::fmt;

use midenc_hir_symbol::Symbol;

use crate::formatter::PrettyPrint;

/// A fully-qualified identifier for the interface being imported, e.g.
/// `namespace::package/interface@version`
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceIdent {
    /// A fully-qualified identifier for the interface being imported, e.g.
    /// `namespace::package/interface@version`
    pub full_name: Symbol,
}

impl InterfaceIdent {
    /// Create a new [InterfaceIdent] from a fully-qualified interface identifier, e.g.
    /// `namespace::package/interface@version`
    pub fn from_full_ident<S: Into<String> + std::fmt::Display>(full_ident: S) -> Self {
        Self {
            full_name: Symbol::intern(full_ident),
        }
    }
}

impl fmt::Debug for InterfaceIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt::Display::fmt(&self.full_name, f)
    }
}
impl fmt::Display for InterfaceIdent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.full_name.as_str().escape_default())
    }
}

/// An identifier for a function in an interface
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceFunctionIdent {
    /// An interface identifier for the interface being imported (e.g.
    /// `namespace::package/interface@version`)
    pub interface: InterfaceIdent,
    /// The name of the function from the interface
    pub function: Symbol,
}

impl InterfaceFunctionIdent {
    /// Create a new [InterfaceFunctionIdent] from a fully-qualified interface
    /// identifier(e.g. `namespace::package/interface@version`) and a function name
    pub fn from_full<S: Into<String> + std::fmt::Display>(interface: S, function: S) -> Self {
        Self {
            interface: InterfaceIdent::from_full_ident(interface),
            function: Symbol::intern(function),
        }
    }
}

impl fmt::Debug for InterfaceFunctionIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} @ {:?}", &self.function, &self.interface)
    }
}
impl fmt::Display for InterfaceFunctionIdent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.pretty_print(f)
    }
}
impl PrettyPrint for InterfaceFunctionIdent {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        flatten(
            const_text("(")
                + display(self.interface)
                + text(format!("#{}", self.function))
                + const_text(")"),
        )
    }
}
