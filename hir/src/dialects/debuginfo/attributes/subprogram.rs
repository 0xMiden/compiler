use alloc::{format, sync::Arc, vec::Vec};

use crate::{
    AttrPrinter, Type, attributes::AttrParser, derive::DialectAttribute,
    dialects::debuginfo::DebugInfoDialect, interner::Symbol, parse::ParserExt, print::AsmPrinter,
};

/// Represents a subprogram (function) scope for debug information.
/// The compile unit is not embedded but typically stored separately on the module.
#[derive(DialectAttribute, Clone, Debug, PartialEq, Eq, Hash)]
#[attribute(dialect = DebugInfoDialect, implements(AttrPrinter))]
pub struct Subprogram {
    pub name: Symbol,
    pub linkage_name: Option<Symbol>,
    pub file: Symbol,
    pub line: u32,
    pub column: Option<u32>,
    pub is_definition: bool,
    pub is_local: bool,
    pub ty: Option<Type>,
    pub param_names: Vec<Symbol>,
}

impl Default for Subprogram {
    fn default() -> Self {
        Self {
            name: crate::interner::symbols::Empty,
            linkage_name: None,
            file: crate::interner::symbols::Empty,
            line: 0,
            column: None,
            is_definition: false,
            is_local: false,
            ty: None,
            param_names: Vec::new(),
        }
    }
}

impl Subprogram {
    pub fn new(name: Symbol, file: Symbol, line: u32, column: Option<u32>) -> Self {
        Self {
            name,
            linkage_name: None,
            file,
            line,
            column,
            is_definition: true,
            is_local: false,
            ty: None,
            param_names: Vec::new(),
        }
    }

    pub fn with_function_type(mut self, ty: crate::FunctionType) -> Self {
        self.ty = Some(Type::Function(Arc::new(ty)));
        self
    }

    pub fn with_param_names<I>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = Symbol>,
    {
        self.param_names = names.into_iter().collect();
        self
    }
}

impl AttrPrinter for SubprogramAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        *printer += const_text("{ ");

        *printer += const_text("name") + const_text(" = ");
        printer.print_string(self.name.as_str());
        *printer += const_text(", ");

        *printer += const_text("file") + const_text(" = ");
        printer.print_string(self.file.as_str());
        *printer += const_text(", ");

        *printer += const_text("line") + const_text(" = ");
        printer.print_decimal_integer(self.line);

        if let Some(column) = self.column {
            *printer += const_text(", ");
            *printer += const_text("column") + const_text(" = ");
            printer.print_decimal_integer(column);
        }

        if let Some(linkage) = self.linkage_name {
            *printer += const_text(", ");
            *printer += const_text("linkage") + const_text(" = ");
            printer.print_string(linkage.as_str());
        }

        if let Some(ty) = &self.ty {
            *printer += const_text(", ");
            *printer += const_text("ty") + const_text(" = ");
            printer.print_type(ty);
        }

        if !self.param_names.is_empty() {
            let names = self
                .param_names
                .iter()
                .map(|name| text(format!("\"{}\"", name.as_str().escape_default())))
                .intersperse(const_text(", "))
                .fold(Document::Empty, |acc, item| acc + item);
            let names = const_text("[") + names + const_text("]");
            *printer += const_text(", ");
            *printer += const_text("params") + const_text(" = ") + names;
        }

        *printer += const_text(", ");
        *printer += const_text("definition") + const_text(" = ");
        printer.print_bool(self.is_definition);

        *printer += const_text(", ");
        *printer += const_text("local") + const_text(" = ");
        printer.print_bool(self.is_local);

        *printer += const_text(" }");
    }
}

impl AttrParser for SubprogramAttr {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        use crate::parse::Token;

        parser.parse_lbrace()?;

        parser.parse_custom_keyword("name")?;
        parser.parse_equal()?;
        let name = parser.parse_string()?.into_inner();
        parser.parse_comma()?;

        parser.parse_custom_keyword("file")?;
        parser.parse_equal()?;
        let file = parser.parse_string()?.into_inner();
        parser.parse_comma()?;

        parser.parse_custom_keyword("line")?;
        parser.parse_equal()?;
        let line = parser.parse_decimal_integer::<u32>()?.into_inner();

        let mut subprogram = Subprogram::new(name.into(), file.into(), line, None);

        while parser.parse_optional_comma()? {
            let (span, prop) = parser
                .token_stream_mut()
                .expect_map("Subprogram property", |tok| match tok {
                    Token::BareIdent(
                        prop @ ("column" | "linkage" | "ty" | "params" | "definition" | "local"),
                    ) => Some(prop),
                    _ => None,
                })?
                .into_parts();
            match prop {
                "column" if subprogram.column.is_none() => {
                    parser.parse_equal()?;
                    subprogram.column = Some(parser.parse_decimal_integer::<u32>()?.into_inner());
                }
                "linkage" if subprogram.linkage_name.is_none() => {
                    parser.parse_equal()?;
                    subprogram.linkage_name = Some(parser.parse_string()?.into_inner().into());
                }
                "ty" if subprogram.ty.is_none() => {
                    parser.parse_equal()?;
                    subprogram.ty = Some(parser.parse_type()?.into_inner());
                }
                "params" if subprogram.param_names.is_empty() => {
                    parser.parse_equal()?;
                    parser.parse_comma_separated_list(
                        crate::parse::Delimiter::OptionalBracket,
                        Some("parameter names"),
                        |parser| {
                            subprogram.param_names.push(parser.parse_string()?.into_inner().into());
                            Ok(true)
                        },
                    )?;
                }
                "definition" => {
                    parser.parse_equal()?;
                    subprogram.is_definition = parser
                        .token_stream_mut()
                        .expect_map("boolean", |tok| match tok {
                            Token::True => Some(true),
                            Token::False => Some(false),
                            _ => None,
                        })?
                        .into_inner();
                }
                "local" => {
                    parser.parse_equal()?;
                    subprogram.is_local = parser
                        .token_stream_mut()
                        .expect_map("boolean", |tok| match tok {
                            Token::True => Some(true),
                            Token::False => Some(false),
                            _ => None,
                        })?
                        .into_inner();
                }
                prop => {
                    return Err(crate::parse::ParserError::InvalidAttributeValue {
                        span,
                        reason: format!("duplicate SubprogramAttr property '{prop}'"),
                    });
                }
            }
        }

        parser.parse_rbrace()?;

        let attr = parser.context_rc().create_attribute::<SubprogramAttr, _>(subprogram);

        Ok(attr.as_attribute_ref())
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::{Subprogram, SubprogramAttr};
    use crate::{
        AttrPrinter, dialects::debuginfo::DebugInfoDialect, interner::Symbol, print::AsmPrinter,
        testing::Test,
    };

    /// Subprogram attrs only appear in function attribute dictionaries, which the custom
    /// function printer does not print, so this round-trip cannot be covered by full-IR lit
    /// tests. Exercise the printer and parser directly instead. This covers the boolean
    /// properties (which must lex as dedicated true/false tokens) and the quoted parameter
    /// name list.
    #[test]
    fn subprogram_attr_print_parse_roundtrip() {
        let test = Test::new("subprogram_attr_print_parse_roundtrip", &[], &[]);
        let context = test.context_rc();
        context.get_or_register_dialect::<DebugInfoDialect>();

        let mut subprogram =
            Subprogram::new(Symbol::intern("fib"), Symbol::intern("test.rs"), 3, Some(5));
        subprogram.linkage_name = Some(Symbol::intern("_ZN3fib"));
        subprogram.is_local = true;
        subprogram = subprogram.with_param_names([Symbol::intern("a"), Symbol::intern("b")]);

        let attr = context.create_attribute::<SubprogramAttr, _>(subprogram.clone());

        let flags = Default::default();
        let mut printer = AsmPrinter::new(context.clone(), &flags);
        attr.borrow().print(&mut printer);
        let printed = printer.finish().to_string();

        let parsed = crate::parse::parse_attribute_for_test::<SubprogramAttr>(context, &printed)
            .unwrap_or_else(|err| {
                panic!("failed to re-parse printed subprogram attr {printed:?}: {err}")
            });
        let parsed = parsed.try_downcast_attr::<SubprogramAttr>().expect("wrong attr type");
        assert_eq!(parsed.borrow().as_value(), &subprogram, "printed form: {printed}");
    }
}
