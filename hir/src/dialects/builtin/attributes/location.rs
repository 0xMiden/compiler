use core::fmt;

use crate::{
    AttrPrinter,
    derive::DialectAttribute,
    diagnostics::{ColumnNumber, LineNumber, SourceId, SourceSpan, Uri},
    dialects::builtin::BuiltinDialect,
    print::AsmPrinter,
};

/// This enumeration represents the various ways in which source locations can be encoded as
/// attributes associated with an operation.
///
/// Typically, source locations are given via [SourceSpan](crate::diagnostics::SourceSpan), but
/// when parsing the IR, other location types are possible.
#[derive(DialectAttribute, Debug, Clone, Default, PartialEq, Eq, Hash)]
#[attribute(
    name = "loc",
    dialect = BuiltinDialect,
    implements(AttrPrinter)
)]
pub enum Location {
    /// Represents an unknown location, i.e. the information is simply unavailable
    #[default]
    Unknown,
    /// Represents the location associated with compiler-generated IR, for which there is no
    /// sensible corresponding source location to use. This is a special case of `Unknown`, but
    /// tells us that it is intentionally unspecified, rather than missing.
    Synthetic,
    /// An opaque location whose concrete representation is not yet known, or is only meaningful
    /// to something that is processing the IR.
    ///
    /// For us, we use this in parsing for location aliases, to represent references to locations
    /// we haven't parsed yet.
    Opaque(usize),
    /// A reference to the first character of the relevant source code in a file, given as the
    /// line and column where the character can be found.
    FileLineCol {
        uri: Uri,
        line: LineNumber,
        column: ColumnNumber,
    },
    /// A reference to a multi-character range comprising the relevant source code in a file, given
    /// as the starting and ending line and column numbers of the selection.
    FileLineColRange {
        uri: Uri,
        start_line: LineNumber,
        start_column: ColumnNumber,
        end_line: LineNumber,
        end_column: ColumnNumber,
    },
}

impl AttrPrinter for LocationAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_trailing_location_specifier(&self.value);
    }
}

impl Location {
    pub fn from_span(span: SourceSpan, context: &crate::Context) -> Self {
        if span.is_unknown() {
            Self::Unknown
        } else if let Some(index) = Self::is_deferred(span) {
            Self::Opaque(index)
        } else if let Ok(file) = context.session().source_manager.get(span.source_id()) {
            let loc = file.location(span);
            if span.is_empty() {
                Self::FileLineCol {
                    uri: loc.uri,
                    line: loc.line,
                    column: loc.column,
                }
            } else {
                let end = file.location(SourceSpan::at(span.source_id(), span.end()));
                Self::FileLineColRange {
                    uri: loc.uri,
                    start_line: loc.line,
                    start_column: loc.column,
                    end_line: end.line,
                    end_column: end.column,
                }
            }
        } else {
            Self::Unknown
        }
    }

    pub fn is_deferred(span: SourceSpan) -> Option<usize> {
        let start = span.start().to_usize();
        if span.source_id() == SourceId::UNKNOWN && start < u32::MAX as usize {
            Some(start)
        } else {
            None
        }
    }

    pub fn deferred(index: u32) -> SourceSpan {
        assert!(index < u32::MAX);
        SourceSpan::at(SourceId::UNKNOWN, index)
    }

    #[cfg(feature = "std")]
    pub fn try_into_span(&self, context: &crate::Context) -> Option<SourceSpan> {
        use crate::diagnostics::SourceManagerExt;
        match self {
            Self::Unknown | Self::Synthetic => Some(SourceSpan::UNKNOWN),
            Self::Opaque(index) => Some(Self::deferred((*index).try_into().unwrap())),
            Self::FileLineCol { uri, line, column } => {
                let path = std::path::Path::new(uri.path());
                let file = context.session().source_manager.load_file(path).ok()?;
                file.line_column_to_span(*line, *column)
            }
            Self::FileLineColRange {
                uri,
                start_line,
                start_column,
                end_line,
                end_column,
            } => {
                let path = std::path::Path::new(uri.path());
                let file = context.session().source_manager.load_file(path).ok()?;
                let start = file.line_column_to_span(*start_line, *start_column)?;
                let end = file.line_column_to_span(*end_line, *end_column)?;
                Some(SourceSpan::new(file.id(), start.start()..end.end()))
            }
        }
    }

    #[cfg(not(feature = "std"))]
    pub fn try_into_span(&self, context: &crate::Context) -> Option<SourceSpan> {
        use crate::diagnostics::SourceManagerExt;
        match self {
            Self::Unknown | Self::Synthetic => Some(SourceSpan::UNKNOWN),
            Self::Opaque(index) => Some(Self::deferred((*index).try_into().unwrap())),
            Self::FileLineCol { uri, line, column } => {
                let file = context.session().source_manager.get_by_uri(uri)?;
                file.line_column_to_span(*line, *column)
            }
            Self::FileLineColRange {
                uri,
                start_line,
                start_column,
                end_line,
                end_column,
            } => {
                let file = context.session().source_manager.get_by_uri(uri)?;
                let start = file.line_column_to_span(*start_line, *start_column)?;
                let end = file.line_column_to_span(*end_line, *end_column)?;
                Some(SourceSpan::new(file.id(), start.start()..end.end()))
            }
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => f.write_str("unknown"),
            Self::Synthetic => f.write_str("synthetic"),
            Self::Opaque(id) => write!(f, "{id}"),
            Self::FileLineCol { uri, line, column } => {
                write!(f, "{uri}:{line}:{column}")
            }
            Self::FileLineColRange {
                uri,
                start_line,
                start_column,
                end_line,
                end_column,
            } => {
                write!(f, "{uri}:{start_line}:{start_column} to {end_line}:{end_column}")
            }
        }
    }
}

impl crate::formatter::PrettyPrint for Location {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        display(self)
    }
}
