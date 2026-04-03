use core::fmt;

use crate::{
    AttrPrinter, AttributeRef, SymbolNameComponent, SymbolPath, SymbolUseRef,
    attributes::AttrParser, derive::DialectAttribute, dialects::builtin::BuiltinDialect,
    print::AsmPrinter,
};

#[derive(DialectAttribute, Debug, Clone, PartialEq, Eq, Hash)]
#[attribute(name = "symbol", dialect = BuiltinDialect, implements(AttrPrinter))]
pub struct SymbolRef {
    /// The referenced path
    path: SymbolPath,
    /// The SymbolUse reference, established when we've linked the referenced operation to this use
    user: Option<SymbolUseRef>,
}

impl Default for SymbolRef {
    fn default() -> Self {
        Self {
            path: SymbolPath::new([SymbolNameComponent::Root]).unwrap(),
            user: None,
        }
    }
}

impl AsRef<SymbolPath> for SymbolRef {
    fn as_ref(&self) -> &SymbolPath {
        &self.path
    }
}

impl AsMut<SymbolPath> for SymbolRef {
    fn as_mut(&mut self) -> &mut SymbolPath {
        &mut self.path
    }
}

impl SymbolRef {
    pub const fn new(path: SymbolPath, user: Option<SymbolUseRef>) -> Self {
        Self { path, user }
    }

    #[inline(always)]
    pub const fn path(&self) -> &SymbolPath {
        &self.path
    }

    #[inline(always)]
    pub fn path_mut(&mut self) -> &mut SymbolPath {
        &mut self.path
    }

    #[inline]
    pub fn set_path(&mut self, path: SymbolPath) {
        self.path = path;
    }

    #[inline(always)]
    pub fn user(&self) -> SymbolUseRef {
        self.user.expect("user has not been initialized")
    }

    pub fn set_user(&mut self, user: SymbolUseRef) {
        assert!(
            self.user.is_none_or(|user| !user.is_linked()),
            "attempted to replace symbol use without unlinking the previously used symbol first"
        );
        self.user = Some(user);
    }
}

impl fmt::Display for SymbolRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.path)
    }
}

impl crate::formatter::PrettyPrint for SymbolRef {
    fn render(&self) -> crate::formatter::Document {
        crate::formatter::display(self)
    }
}

impl SymbolRefAttr {
    #[inline(always)]
    pub const fn path(&self) -> &SymbolPath {
        self.value.path()
    }

    #[inline]
    pub fn set_path(&mut self, path: SymbolPath) {
        self.value.set_path(path);
    }

    #[inline(always)]
    pub fn user(&self) -> SymbolUseRef {
        self.value.user()
    }

    pub fn set_user(&mut self, user: SymbolUseRef) {
        self.value.set_user(user);
    }

    /// Resolves this symbol reference relative to the operation that owns the tracked use.
    ///
    /// This immutably borrows the tracked user while resolving the containing symbol table. Callers
    /// must not hold a mutable borrow of that user when invoking this API, or the borrow check will
    /// report an aliasing violation.
    pub fn resolve(&self) -> Option<crate::SymbolRef> {
        self.user().borrow().resolve_symbol(self.path())
    }

    /// Unlinks the tracked use from its current symbol, if it is linked.
    pub fn unlink(&mut self) -> Option<crate::SymbolRef> {
        let mut user = self.user();
        let path = self.path().clone();
        user.borrow_mut().unlink_from_symbol(user, &path)
    }

    /// Links the tracked use to `symbol` without modifying the stored path.
    pub fn link(&mut self, symbol: crate::SymbolRef) {
        let mut user = self.user();
        user.borrow_mut().link_to_symbol(user, symbol);
    }

    /// Updates this symbol reference to point at `symbol`, mutating and relinking the existing use.
    pub fn set_symbol(&mut self, symbol: crate::SymbolRef) {
        self.unlink();
        self.set_path(symbol.borrow().path());
        self.link(symbol);
    }
}

impl AttrPrinter for SymbolRefAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_symbol_path(&self.path);
    }
}

impl AttrParser for SymbolRefAttr {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        parser.parse_symbol_ref().map(|spanned| spanned.into_inner() as AttributeRef)
    }
}
