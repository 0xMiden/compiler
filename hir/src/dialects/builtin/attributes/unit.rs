use crate::{
    AttrPrinter, derive::DialectAttribute, dialects::builtin::BuiltinDialect, print::AsmPrinter,
};

#[derive(DialectAttribute, Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
#[attribute(dialect = BuiltinDialect, implements(AttrPrinter))]
pub struct Unit;

impl AsRef<()> for Unit {
    fn as_ref(&self) -> &() {
        &()
    }
}

impl From<()> for Unit {
    fn from(_value: ()) -> Self {
        Unit
    }
}

impl From<Unit> for () {
    fn from(_value: Unit) -> Self {}
}

impl core::fmt::Display for Unit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("unit")
    }
}

impl AttrPrinter for UnitAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_keyword("unit");
    }
}
