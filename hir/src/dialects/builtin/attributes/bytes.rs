use alloc::sync::Arc;

use crate::{
    AttrPrinter, constants::ConstantData, derive::DialectAttribute,
    dialects::builtin::BuiltinDialect,
};

#[derive(DialectAttribute)]
#[attribute(
    dialect = BuiltinDialect,
    remote = "Arc<ConstantData>",
    implements(AttrPrinter)
)]
pub struct Bytes;

impl AttrPrinter for BytesAttr {
    fn print(&self, printer: &mut crate::print::AsmPrinter<'_>) {
        use alloc::string::ToString;

        printer.print_string(self.value.to_string());
    }
}
