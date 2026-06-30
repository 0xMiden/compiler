use alloc::format;

use crate::{
    AttrPrinter, Type, attributes::AttrParser, derive::DialectAttribute,
    dialects::debuginfo::DebugInfoDialect, interner::Symbol, parse::ParserExt, print::AsmPrinter,
};

/// Represents a local variable debug record.
/// The scope (Subprogram) is not embedded but instead stored on the containing function.
#[derive(DialectAttribute, Clone, Debug, PartialEq, Eq, Hash)]
#[attribute(dialect = DebugInfoDialect, implements(AttrPrinter))]
pub struct Variable {
    pub name: Symbol,
    pub arg_index: Option<u32>,
    pub file: Symbol,
    pub line: u32,
    pub column: Option<u32>,
    pub ty: Option<Type>,
}

impl Default for Variable {
    fn default() -> Self {
        Self {
            name: crate::interner::symbols::Empty,
            arg_index: None,
            file: crate::interner::symbols::Empty,
            line: 0,
            column: None,
            ty: None,
        }
    }
}

impl Variable {
    pub fn new(name: Symbol, file: Symbol, line: u32, column: Option<u32>) -> Self {
        Self {
            name,
            arg_index: None,
            file,
            line,
            column,
            ty: None,
        }
    }
}

impl AttrPrinter for VariableAttr {
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

        if let Some(arg_index) = self.arg_index {
            *printer += const_text(", ");
            *printer += const_text("arg") + const_text(" = ");
            printer.print_decimal_integer(arg_index);
        }

        if let Some(ty) = &self.ty {
            *printer += const_text(", ");
            *printer += const_text("ty") + const_text(" = ");
            printer.print_type(ty);
        }

        *printer += const_text(" }");
    }
}

impl AttrParser for VariableAttr {
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

        let mut var = Variable::new(name.into(), file.into(), line, None);

        while parser.parse_optional_comma()? {
            let (span, prop) = parser
                .token_stream_mut()
                .expect_map("DILocalVariable property", |tok| match tok {
                    Token::BareIdent(prop @ ("column" | "arg" | "ty")) => Some(prop),
                    _ => None,
                })?
                .into_parts();
            match prop {
                "column" if var.column.is_none() => {
                    parser.parse_equal()?;
                    var.column = Some(parser.parse_decimal_integer::<u32>()?.into_inner());
                }
                "arg" if var.arg_index.is_none() => {
                    parser.parse_equal()?;
                    var.column = Some(parser.parse_decimal_integer::<u32>()?.into_inner());
                }
                "ty" if var.ty.is_none() => {
                    parser.parse_equal()?;
                    var.ty = Some(parser.parse_type()?.into_inner());
                }
                prop => {
                    return Err(crate::parse::ParserError::InvalidAttributeValue {
                        span,
                        reason: format!("duplicate DILocalVariableAttr property '{prop}'"),
                    });
                }
            }
        }

        parser.parse_rbrace()?;

        let attr = parser.context_rc().create_attribute::<VariableAttr, _>(var);

        Ok(attr.as_attribute_ref())
    }
}
