use crate::{
    AttrPrinter, derive::DialectAttribute, dialects::builtin::BuiltinDialect, print::AsmPrinter,
};

#[derive(DialectAttribute, Debug, Clone, PartialEq, Eq, Hash)]
#[attribute(
    dialect = BuiltinDialect,
    remote = "crate::Type",
    default = "default_type",
    implements(AttrPrinter),
)]
#[allow(unused)]
struct Type;

impl AttrPrinter for TypeAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_type(&self.value);
    }
}

#[derive(DialectAttribute, Debug, Clone, PartialEq, Eq, Hash)]
#[attribute(
    dialect = BuiltinDialect,
    remote = "crate::FunctionType",
    default = "default_function_type",
    implements(AttrPrinter)
)]
#[allow(unused)]
struct FunctionType;

impl AttrPrinter for FunctionTypeAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_function_type(&self.value);
    }
}

const fn default_type() -> crate::Type {
    crate::Type::Unknown
}

fn default_function_type() -> crate::FunctionType {
    crate::FunctionType::new(crate::CallConv::SystemV, [], [])
}
