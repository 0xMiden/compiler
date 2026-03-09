use alloc::{format, string::String, vec, vec::Vec};

use crate::{
    BlockId, CompactString, Type, ValueId,
    diagnostics::{Diagnostic, LabeledSpan, RelatedError, Report, SourceSpan, miette},
    formatter::DisplayValues,
    interner,
};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum ParserError {
    #[error("unexpected end of input: expected one of {}", DisplayValues::new(expected.iter()))]
    #[diagnostic()]
    UnexpectedEof { expected: Vec<String> },
    #[error("invalid syntax")]
    #[diagnostic()]
    UnexpectedToken {
        #[label("unexpected token '{token}'")]
        span: SourceSpan,
        token: String,
        #[help]
        expected: Option<String>,
    },
    #[error("invalid syntax")]
    #[diagnostic()]
    InvalidCharacter {
        #[label("unexpected character '{character}'")]
        span: SourceSpan,
        character: char,
    },
    #[error("invalid syntax")]
    #[diagnostic()]
    UnclosedQuote {
        #[label("missing closing quote for string starting here")]
        span: SourceSpan,
    },
    #[error("invalid syntax")]
    #[diagnostic()]
    UnclosedDelimiter {
        #[label("missing closing delimiter '{expected}'")]
        span: SourceSpan,
        expected: char,
    },
    #[error("invalid syntax")]
    #[diagnostic(help("Only '\"', '\\r', '\\n', and '\\t' may be escaped in strings"))]
    InvalidEscapeSequence {
        #[label("invalid escape sequence")]
        span: SourceSpan,
    },
    #[error("invalid integer literal")]
    #[diagnostic()]
    InvalidIntegerLiteral {
        #[label("{reason}")]
        span: SourceSpan,
        reason: String,
    },
    #[error("invalid string")]
    #[diagnostic()]
    InvalidString {
        #[label("{reason}")]
        span: SourceSpan,
        reason: String,
    },
    #[error("invalid attribute value")]
    #[diagnostic()]
    InvalidAttributeValue {
        #[label("{reason}")]
        span: SourceSpan,
        reason: String,
    },
    #[error("invalid operand and type list")]
    #[diagnostic()]
    OperandAndTypeListMismatch {
        #[label(
            "there are {num_operands} operands, but {num_types} types: expected the same number \
             of both"
        )]
        span: SourceSpan,
        num_operands: usize,
        num_types: usize,
    },
    #[error("use of undeclared values")]
    #[diagnostic()]
    UndeclaredValueUses {
        #[label(collection)]
        labels: Vec<LabeledSpan>,
    },
    #[error("operation location alias was never defined")]
    #[diagnostic()]
    UnresolvedLocationAlias {
        #[label("occurs here")]
        span: SourceSpan,
    },
    #[error("invalid location alias value")]
    #[diagnostic()]
    InvalidLocationAlias {
        #[label("{reason}")]
        span: SourceSpan,
        reason: String,
    },
    #[error("use of undefined blocks")]
    #[diagnostic()]
    UndefinedBlocks {
        #[label(collection)]
        labels: Vec<LabeledSpan>,
    },
    #[error("redefinition of SSA value")]
    #[diagnostic()]
    ValueRedefinition {
        #[label(primary, "occurs here")]
        span: SourceSpan,
        #[label("previously defined here")]
        prev_span: SourceSpan,
    },
    #[error("definition of SSA value does not match expected type")]
    #[diagnostic()]
    ValueDefinitionTypeMismatch {
        #[label(primary, "definition here has type {ty}")]
        span: SourceSpan,
        #[label("previously used here with type {prev_ty}")]
        prev_span: SourceSpan,
        ty: Type,
        prev_ty: Type,
    },
    #[error("invalid use of result number in argument list")]
    #[diagnostic()]
    ResultNumberUsedInArgumentList {
        #[label(primary)]
        span: SourceSpan,
        #[label]
        value_span: SourceSpan,
    },
    #[error("invalid result index")]
    #[diagnostic()]
    InvalidResultIndex {
        #[label(primary, "{reason}")]
        span: SourceSpan,
        #[label]
        value_span: SourceSpan,
        reason: String,
    },
    #[error("use of value expects different type than prior uses")]
    #[diagnostic()]
    ValueUseTypeMismatch {
        #[label(primary, "use here expects {ty}")]
        span: SourceSpan,
        #[label("previous use here expects {prev_ty}")]
        prev_span: SourceSpan,
        ty: Type,
        prev_ty: Type,
    },
    #[error("mismatched value and type lists")]
    #[diagnostic()]
    MismatchedValueAndTypeLists {
        #[label("there are {num_values} values, but {num_types} types")]
        span: SourceSpan,
        num_values: usize,
        num_types: usize,
    },
    #[error("invalid result count")]
    #[diagnostic()]
    InvalidResultCount { span: SourceSpan },
    #[error("named op with no results")]
    #[diagnostic()]
    NamedOpWithNoResults { span: SourceSpan },
    #[error("result count mismatch")]
    #[diagnostic()]
    ResultCountMismatch {
        span: SourceSpan,
        count: usize,
        expected: u8,
    },
    #[error("invalid successor list")]
    #[diagnostic()]
    InvalidEmptySuccessorList {
        #[label("expected at least one successor")]
        span: SourceSpan,
    },
    #[error("invalid operation name")]
    #[diagnostic()]
    InvalidOperationName {
        #[label("{reason}")]
        span: SourceSpan,
        reason: String,
    },
    #[error("invalid operation")]
    #[diagnostic()]
    NonTerminatorWithSuccessors {
        #[label("non-terminator operations cannot have successors")]
        span: SourceSpan,
    },
    #[error("invalid operation type")]
    #[diagnostic()]
    InvalidOperationType {
        #[label(primary)]
        span: SourceSpan,
        #[label("{reason}")]
        ty_span: SourceSpan,
        reason: String,
    },
    #[error("invalid operation")]
    #[diagnostic()]
    UnknownOperation {
        #[label("unknown/unregistered operation")]
        span: SourceSpan,
    },
    #[error("invalid attribute")]
    #[diagnostic()]
    UnknownAttribute {
        #[label("unknown/unregistered attribute")]
        span: SourceSpan,
    },
    #[error("invalid custom operation")]
    #[diagnostic()]
    InvalidCustomOperation {
        #[label("{reason}")]
        span: SourceSpan,
        reason: String,
    },
    #[error("invalid block name in region with named arguments")]
    #[diagnostic()]
    BlockNameInRegionWithNamedArgs {
        #[label]
        span: SourceSpan,
    },
    #[error("region entry argument is already in use")]
    #[diagnostic()]
    RegionArgumentAlreadyDefined {
        #[label(primary, "attempted to redefine {arg} here")]
        span: SourceSpan,
        #[label("previously defined here")]
        prev_span: SourceSpan,
        arg: ValueId,
    },
    #[error("entry block arguments were already defined")]
    #[diagnostic()]
    EntryBlockArgumentsAlreadyDefined {
        #[label]
        span: SourceSpan,
    },
    #[error("redefinition of block {name}")]
    #[diagnostic()]
    BlockAlreadyDefined {
        #[label]
        span: SourceSpan,
        name: BlockId,
    },
    #[error("too many arguments specified in argument list")]
    #[diagnostic()]
    TooManyBlockArguments {
        #[label]
        span: SourceSpan,
    },
    #[error("block argument type mismatch")]
    #[diagnostic()]
    BlockArgumentTypeMismatch {
        #[label("expected {expected}, got {ty}")]
        span: SourceSpan,
        arg: ValueId,
        ty: Type,
        expected: Type,
    },
    #[error("attribute '{name}' occurs more than once in the attribute list")]
    #[diagnostic()]
    DuplicateAttribute {
        #[label]
        span: SourceSpan,
        name: interner::Symbol,
    },
    #[error("invalid attribute alias definition")]
    #[diagnostic()]
    AttributeAliasAlreadyDefined {
        #[label("alias '{id}' is already defined")]
        span: SourceSpan,
        id: interner::Symbol,
    },
    #[error("invalid attribute alias name '{id}'")]
    #[diagnostic()]
    InvalidAttributeAliasName {
        #[label("{reason}")]
        span: SourceSpan,
        id: interner::Symbol,
        reason: String,
    },
    #[error("invalid type alias definition")]
    #[diagnostic()]
    TypeAliasAlreadyDefined {
        #[label("alias '{id}' is already defined")]
        span: SourceSpan,
        id: interner::Symbol,
    },
    #[error("invalid type alias name '{id}'")]
    #[diagnostic()]
    InvalidTypeAliasName {
        #[label("{reason}")]
        span: SourceSpan,
        id: interner::Symbol,
        reason: String,
    },
    #[error("invalid file metadata")]
    UnknownFileMetadata {
        #[label("'{key}' is not a recognized metadata key")]
        span: SourceSpan,
        key: CompactString,
    },
    #[error(transparent)]
    #[diagnostic(transparent)]
    Report(#[from] RelatedError),
}

impl From<Report> for ParserError {
    #[inline]
    fn from(value: Report) -> Self {
        Self::Report(RelatedError::new(value))
    }
}
