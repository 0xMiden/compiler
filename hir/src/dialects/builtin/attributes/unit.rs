use crate::{
    AttrPrinter, attributes::Marker, derive::DialectAttribute, dialects::builtin::BuiltinDialect,
    print::AsmPrinter,
};

type UnitValue = ();

#[derive(DialectAttribute)]
#[attribute(dialect = BuiltinDialect, remote = "UnitValue", implements(AttrPrinter, Marker))]
#[allow(unused)]
struct Unit;

impl AttrPrinter for UnitAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_keyword("unit");
    }
}
