use core::fmt;

use crate::{
    AttrPrinter, SymbolNameComponent, SymbolPath, SymbolUseRef, derive::DialectAttribute,
    dialects::builtin::BuiltinDialect, print::AsmPrinter,
};

#[derive(DialectAttribute, Debug, Clone, PartialEq, Eq, Hash)]
#[attribute(name = "symbol", dialect = BuiltinDialect, implements(AttrPrinter))]
pub struct SymbolRef {
    /// The referenced path
    path: SymbolPath,
    /// The SymbolUse reference, established when we've linked the referenced operation to this use
    ///
    /// This is guaranteed to be a valid reference when attached to an operation that has been
    /// built - otherwise it is possibly dangling.
    user: SymbolUseRef,
}

impl Default for SymbolRef {
    fn default() -> Self {
        Self {
            path: SymbolPath::new([SymbolNameComponent::Root]).unwrap(),
            user: SymbolUseRef::dangling(),
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
    pub const fn new(path: SymbolPath, user: SymbolUseRef) -> Self {
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
    pub const fn user(&self) -> SymbolUseRef {
        self.user
    }

    pub fn set_user(&mut self, user: SymbolUseRef) {
        assert!(
            !self.user.is_linked(),
            "attempted to replace symbol use without unlinking the previously used symbol first"
        );
        self.user = user;
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
    pub const fn user(&self) -> SymbolUseRef {
        self.value.user()
    }

    pub fn set_user(&mut self, user: SymbolUseRef) {
        self.value.set_user(user);
    }
}

impl AttrPrinter for SymbolRefAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_symbol_path(&self.path);
    }
}
