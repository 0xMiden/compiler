use crate::{
    AttrPrinter, Immediate, attributes::IntegerLikeAttr, derive::DialectAttribute,
    dialects::builtin::BuiltinDialect, print::AsmPrinter,
};

#[derive(DialectAttribute)]
#[attribute(
    dialect = BuiltinDialect,
    remote = "bool",
    implements(IntegerLikeAttr, AttrPrinter)
)]
#[allow(unused)]
struct Bool;

impl IntegerLikeAttr for BoolAttr {
    #[inline]
    fn as_immediate(&self) -> Immediate {
        Immediate::I1(self.value)
    }

    fn set_from_immediate_lossy(&mut self, value: Immediate) {
        // Treat non-zero values as truthy
        self.value = value.as_bool().unwrap_or(true);
    }
}

impl AttrPrinter for BoolAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_bool(self.value);
    }
}
