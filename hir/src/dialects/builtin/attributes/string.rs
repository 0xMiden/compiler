use crate::{
    AttrPrinter, derive::DialectAttribute, dialects::builtin::BuiltinDialect, print::AsmPrinter,
};

#[derive(DialectAttribute)]
#[attribute(
    dialect = BuiltinDialect,
    remote = "crate::CompactString",
    implements(AttrPrinter),
)]
#[allow(unused)]
struct String;

impl AttrPrinter for StringAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_string(self.value.as_str());
    }
}
