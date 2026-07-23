use alloc::vec::Vec;

use crate::{
    AttrPrinter, attributes::AttrParser, derive::DialectAttribute,
    dialects::debuginfo::DebugInfoDialect, interner::Symbol, parse::ParserExt, print::AsmPrinter,
};

pub const INLINE_CALL_CHAIN_ATTR_NAME: &str = "di.inline_call_chain";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InlineCallFrame {
    pub name: Symbol,
    pub linkage_name: Option<Symbol>,
    pub file: Symbol,
    pub line: u32,
    pub column: u32,
    pub call_file: Symbol,
    pub call_line: u32,
    pub call_column: u32,
}

#[derive(DialectAttribute, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[attribute(dialect = DebugInfoDialect, implements(AttrPrinter))]
pub struct InlineCallChain {
    pub frames: Vec<InlineCallFrame>,
}

impl InlineCallChain {
    pub fn new(frames: Vec<InlineCallFrame>) -> Self {
        Self { frames }
    }
}

impl AttrPrinter for InlineCallChainAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        *printer += const_text("[");
        for (index, frame) in self.frames.iter().enumerate() {
            if index > 0 {
                *printer += const_text(", ");
            }
            *printer += const_text("{ name = ");
            printer.print_string(frame.name.as_str());
            *printer += const_text(", linkage = ");
            printer.print_string(frame.linkage_name.map(|name| name.as_str()).unwrap_or_default());
            *printer += const_text(", file = ");
            printer.print_string(frame.file.as_str());
            *printer += const_text(", line = ");
            printer.print_decimal_integer(frame.line);
            *printer += const_text(", column = ");
            printer.print_decimal_integer(frame.column);
            *printer += const_text(", call_file = ");
            printer.print_string(frame.call_file.as_str());
            *printer += const_text(", call_line = ");
            printer.print_decimal_integer(frame.call_line);
            *printer += const_text(", call_column = ");
            printer.print_decimal_integer(frame.call_column);
            *printer += const_text(" }");
        }
        *printer += const_text("]");
    }
}

impl AttrParser for InlineCallChainAttr {
    fn parse(
        parser: &mut dyn crate::parse::Parser<'_>,
    ) -> crate::parse::ParseResult<crate::AttributeRef> {
        use crate::parse::Delimiter;

        let mut frames = Vec::new();
        parser.parse_comma_separated_list(
            Delimiter::Bracket,
            Some("inline call chain"),
            |parser| {
                parser.parse_lbrace()?;
                parser.parse_custom_keyword("name")?;
                parser.parse_equal()?;
                let name = parser.parse_string()?.into_inner();
                parser.parse_comma()?;
                parser.parse_custom_keyword("linkage")?;
                parser.parse_equal()?;
                let linkage = parser.parse_string()?.into_inner();
                parser.parse_comma()?;
                parser.parse_custom_keyword("file")?;
                parser.parse_equal()?;
                let file = parser.parse_string()?.into_inner();
                parser.parse_comma()?;
                parser.parse_custom_keyword("line")?;
                parser.parse_equal()?;
                let line = parser.parse_decimal_integer::<u32>()?.into_inner();
                parser.parse_comma()?;
                parser.parse_custom_keyword("column")?;
                parser.parse_equal()?;
                let column = parser.parse_decimal_integer::<u32>()?.into_inner();
                parser.parse_comma()?;
                parser.parse_custom_keyword("call_file")?;
                parser.parse_equal()?;
                let call_file = parser.parse_string()?.into_inner();
                parser.parse_comma()?;
                parser.parse_custom_keyword("call_line")?;
                parser.parse_equal()?;
                let call_line = parser.parse_decimal_integer::<u32>()?.into_inner();
                parser.parse_comma()?;
                parser.parse_custom_keyword("call_column")?;
                parser.parse_equal()?;
                let call_column = parser.parse_decimal_integer::<u32>()?.into_inner();
                parser.parse_rbrace()?;

                frames.push(InlineCallFrame {
                    name: name.into(),
                    linkage_name: (!linkage.is_empty()).then(|| linkage.into()),
                    file: file.into(),
                    line,
                    column,
                    call_file: call_file.into(),
                    call_line,
                    call_column,
                });
                Ok(true)
            },
        )?;

        Ok(parser
            .context_rc()
            .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(frames))
            .as_attribute_ref())
    }
}
