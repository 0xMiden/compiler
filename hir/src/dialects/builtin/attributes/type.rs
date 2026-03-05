use crate::{
    AttrPrinter, attributes::AttrParser, derive::DialectAttribute,
    dialects::builtin::BuiltinDialect, parse::Token, print::AsmPrinter,
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

impl AttrParser for TypeAttr {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        let ty = if parser.token_stream_mut().is_next(|tok| matches!(tok, Token::Lparen)) {
            crate::Type::Function(parser.parse_function_type()?.into_inner().into())
        } else {
            parser.parse_non_function_type()?.into_inner()
        };

        Ok(parser.context_rc().create_attribute::<TypeAttr, _>(ty))
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
