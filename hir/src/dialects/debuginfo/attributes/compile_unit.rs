use crate::{
    AttrPrinter, attributes::AttrParser, derive::DialectAttribute,
    dialects::debuginfo::DebugInfoDialect, interner::Symbol, print::AsmPrinter,
};

/// Represents the compilation unit associated with debug information.
///
/// The fields in this struct are intentionally aligned with the subset of
/// DWARF metadata we currently care about when tracking variable locations.
#[derive(DialectAttribute, Clone, Debug, PartialEq, Eq, Hash)]
#[attribute(dialect = DebugInfoDialect, implements(AttrPrinter))]
pub struct CompileUnit {
    pub language: Symbol,
    pub file: Symbol,
    pub directory: Option<Symbol>,
    pub producer: Option<Symbol>,
    pub optimized: bool,
}

impl Default for CompileUnit {
    fn default() -> Self {
        Self {
            language: crate::interner::symbols::Empty,
            file: crate::interner::symbols::Empty,
            directory: None,
            producer: None,
            optimized: false,
        }
    }
}

impl CompileUnit {
    pub fn new(language: Symbol, file: Symbol) -> Self {
        Self {
            language,
            file,
            directory: None,
            producer: None,
            optimized: false,
        }
    }
}

impl AttrPrinter for CompileUnitAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        *printer += const_text("{ ");

        *printer += const_text("language") + const_text(" = ");
        printer.print_string(self.language.as_str());
        *printer += const_text(", ");

        *printer += const_text("file") + const_text(" = ");
        printer.print_string(self.file.as_str());

        if let Some(directory) = self.directory {
            *printer += const_text(", ");
            *printer += const_text("directory") + const_text(" = ");
            printer.print_string(directory.as_str());
        }

        if let Some(producer) = self.producer {
            *printer += const_text(", ");
            *printer += const_text("producer") + const_text(" = ");
            printer.print_string(producer.as_str());
        }

        *printer += const_text(", ");
        *printer += const_text("optimized") + const_text(" = ");
        printer.print_bool(self.optimized);

        *printer += const_text(" }");
    }
}

impl AttrParser for CompileUnitAttr {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        use crate::parse::Token;

        parser.parse_lbrace()?;

        parser.parse_custom_keyword("language")?;
        parser.parse_equal()?;
        let language = parser.parse_string()?.into_inner();
        parser.parse_comma()?;

        parser.parse_custom_keyword("file")?;
        parser.parse_equal()?;
        let file = parser.parse_string()?.into_inner();
        parser.parse_comma()?;

        let mut unit = CompileUnit::new(language.into(), file.into());

        if parser.parse_optional_custom_keyword("directory")?.is_some() {
            parser.parse_equal()?;
            unit.directory = Some(parser.parse_string()?.into_inner().into());
            parser.parse_comma()?;
        }
        if parser.parse_optional_custom_keyword("producer")?.is_some() {
            parser.parse_equal()?;
            unit.producer = Some(parser.parse_string()?.into_inner().into());
            parser.parse_comma()?;
        }
        if parser.parse_optional_custom_keyword("optimized")?.is_some() {
            parser.parse_equal()?;
            unit.optimized = parser
                .token_stream_mut()
                .expect_map("boolean", |tok| match tok {
                    Token::True => Some(true),
                    Token::False => Some(false),
                    _ => None,
                })?
                .into_inner();
        }

        parser.parse_rbrace()?;

        let attr = parser.context_rc().create_attribute::<CompileUnitAttr, _>(unit);

        Ok(attr.as_attribute_ref())
    }
}
