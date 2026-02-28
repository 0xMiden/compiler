use crate::{
    AttrPrinter, Immediate,
    attributes::{BoolLikeAttr, IntegerLikeAttr},
    derive::DialectAttribute,
    dialects::builtin::BuiltinDialect,
    print::AsmPrinter,
};

#[derive(DialectAttribute)]
#[attribute(
    dialect = BuiltinDialect,
    remote = "bool",
    implements(BoolLikeAttr, IntegerLikeAttr, AttrPrinter)
)]
#[allow(unused)]
struct Bool;

impl BoolLikeAttr for BoolAttr {
    #[inline(always)]
    fn as_bool(&self) -> bool {
        self.value
    }

    #[inline(always)]
    fn set_bool(&mut self, value: bool) {
        self.value = value;
    }
}

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
