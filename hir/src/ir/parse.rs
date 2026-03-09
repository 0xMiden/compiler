#![allow(unused)]

macro_rules! span {
    ($source_id:expr, $at:expr) => {
        SourceSpan::at($source_id, $at)
    };

    ($source_id:expr, $start:expr, $end:expr) => {
        SourceSpan::new($source_id, ($start)..($end))
    };
}

macro_rules! spanned {
    ($source_id:expr, $at:expr, $value:expr) => {
        Span::new(span!($source_id, $at), $value)
    };

    ($source_id:expr, $start:expr, $end:expr, $value:expr) => {
        Span::new(span!($source_id, $start, $end), $value)
    };
}

mod asm_parser;
mod delimiter;
mod error;
mod from_str_radix;
pub mod lexer;
mod operation;
mod parser;
pub mod scanner;
#[cfg(test)]
mod tests;
mod token;

use alloc::{
    boxed::Box,
    format,
    rc::Rc,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::{
    num::NonZeroU8,
    ops::{Deref, DerefMut},
};

use miden_core::Felt;
use midenc_hir_type::{AddressSpace, PointerType};
use midenc_session::{
    SourceManager,
    diagnostics::{
        ColumnNumber, Diagnostic, FileLineCol, LabeledSpan, LineNumber, RelatedError, Report,
        Severity, SourceFile, SourceId, SourceManagerExt, SourceSpan, Span, Uri, WrapErr,
        miette::{self, diagnostic},
    },
};

use self::{
    asm_parser::AsmParserState,
    lexer::{Lexed, Lexer, TokenStream},
    parser::DefaultParser,
    scanner::Scanner,
};
pub use self::{
    delimiter::Delimiter,
    error::ParserError,
    from_str_radix::FromStrRadix,
    parser::{Parser, ParserExt},
    token::Token,
};
use super::{
    AsCallableSymbolRef, BlockArgumentRef, BlockId, BlockRef, Context, GenericOperationBuilder,
    Ident, OpBuilder, OpRegistration, OpResultRef, Operation, OperationName, OperationRef,
    OperationState, RawEntityRef, RegionRef, SuccessorInfo, SymbolRef, Type,
    UnsafeIntrusiveEntityRef, ValueId, ValueRef, interner::Symbol, operation::ParseAssemblyFn,
};
use crate::{
    Attribute, AttributeRef, Builder, CallableOpInterface, CompactString, FunctionType, FxHashMap,
    NamedAttribute, ProgramPoint, SmallVec, ToCompactString,
    adt::{SmallOrdMap, smallmap::SmallMap},
    dialects::builtin::{
        WorldBuilder, WorldRef,
        attributes::{Location, LocationAttr},
    },
    formatter::DisplayValues,
    interner,
    print::TypePrinter,
    smallvec,
};

/// The interface for parsing operations printed to IR assembly
pub trait OpParser {
    /// Parse the current operation into `state` using `parser`.
    ///
    /// The following may be assumed by the parser implementation:
    ///
    /// 1. The parser has already consumed all operation results and the operation name, and it is
    ///    up to this trait function to parse everything after that point.
    /// 2. The trailing location specifier and terminating `;` will be handled by the parser, and
    ///    do not need to be parsed by this function.
    ///
    fn parse(state: &mut OperationState, parser: &mut dyn OpAsmParser<'_>) -> ParseResult;
}

pub type ParseResult<T = ()> = Result<T, ParserError>;

/// This struct contains configuration for the IR assembly parser.
pub struct ParserConfig {
    /// The context in which IR entities should be constructed
    pub context: Rc<Context>,
    /// Set to true if the parser should verify after parsing
    pub verify: bool,
}

impl ParserConfig {
    pub fn new(context: Rc<Context>) -> Self {
        Self {
            context,
            verify: true,
        }
    }

    /// Set the flag that determines whether verification will be run after parsing
    pub fn verify_after_parse(mut self, yes: bool) -> Self {
        self.verify = yes;
        self
    }

    #[inline]
    pub const fn should_verify_after_parse(&self) -> bool {
        self.verify
    }
}

/// This struct contains the state of the parser relevant to high-level parser implementations
pub struct ParserState<'input> {
    pub config: ParserConfig,
    pub token_stream: TokenStream<'input>,
    /// The current state for symbol parsing
    pub symbols: SymbolState,
    /// Optional high-level parser state to be populated during parsing
    pub asm_state: Option<Box<AsmParserState>>,
    /// Contains the stack of default dialects to use when parsing regions.
    ///
    /// A new dialect gets pushed to the stack before parsing regions nested under an operation
    /// implementing `OpAsmOpInterface`, and popped when done. At the top-level we start with
    /// "builtin" as the default, so that the top-level builtin operations parse as-is.
    pub default_dialect_stack: SmallVec<[interner::Symbol; 1]>,
}

impl<'input> ParserState<'input> {
    pub fn new(config: ParserConfig, token_stream: TokenStream<'input>) -> Self {
        Self {
            config,
            token_stream,
            symbols: Default::default(),
            asm_state: None,
            default_dialect_stack: smallvec![interner::Symbol::intern("builtin")],
        }
    }

    #[inline]
    pub fn context(&self) -> &Context {
        &self.config.context
    }

    #[inline]
    pub fn context_rc(&self) -> Rc<Context> {
        self.config.context.clone()
    }
}

/// This struct records all parsed top-level symbols
#[derive(Default)]
pub struct SymbolState {
    /// A map from attribute alias identifier to Attribute
    pub attribute_alias_definitions: FxHashMap<interner::Symbol, Span<AttributeRef>>,
    /// A map from type alias identifier to Type
    pub type_alias_definitions: FxHashMap<interner::Symbol, Span<Type>>,
}

/// Parse IR assembly in the generic format from `source` with the provided `uri` and `config`
pub fn parse_generic(
    config: ParserConfig,
    uri: Uri,
    source: impl Into<String>,
) -> Result<WorldRef, Report> {
    use midenc_session::diagnostics::SourceLanguage;
    let source_manager = &config.context.session().source_manager;
    let source_file = source_manager.load(SourceLanguage::Other("hir"), uri, source.into());
    parse_source_generic(config, source_file)
}

/// Parse IR assembly in the generic format from `path` with the provided `config`
#[cfg(feature = "std")]
pub fn parse_file_generic(
    config: ParserConfig,
    path: impl AsRef<std::path::Path>,
) -> Result<WorldRef, Report> {
    let source_manager = &config.context.session().source_manager;
    let source_file = source_manager.load_file(path.as_ref()).map_err(Report::msg)?;
    parse_source_generic(config, source_file)
}

/// This parses the given source file and appends parsed operations to a new [World].
///
/// If parsing is successful, the populated [World] is returned. Otherwise, the error that caused
/// parsing to fail is returned.
fn parse_source_generic(
    config: ParserConfig,
    source_file: Arc<SourceFile>,
) -> Result<WorldRef, Report> {
    let source = source_file.as_str();
    let scanner = Scanner::new(source);
    let token_stream = TokenStream::new(source_file.id(), scanner);
    let mut parser = DefaultParser::new(ParserState::new(config, token_stream));
    parser.state_mut().asm_state = Some(Default::default());

    let span = parser.current_location();
    let mut operation_parser = operation::TopLevelOperationParser::new(parser);
    operation_parser
        .parse(span)
        .map_err(|err| Report::from(err).with_source_code(source_file.clone()))
}

/// Parse an operation of type `T` from `source` with the provided `uri` and `config`
pub fn parse<T: OpParser + OpRegistration>(
    config: ParserConfig,
    uri: Uri,
    source: impl Into<String>,
) -> Result<UnsafeIntrusiveEntityRef<T>, Report> {
    use midenc_session::diagnostics::SourceLanguage;
    let source_manager = &config.context.session().source_manager;
    let source_file = source_manager.load(SourceLanguage::Other("hir"), uri, source.into());
    parse_source(config, source_file)
}

/// Parse an operation of type `T` from `path` with the provided `config`
#[cfg(feature = "std")]
pub fn parse_file<T: OpParser + OpRegistration>(
    config: ParserConfig,
    path: impl AsRef<std::path::Path>,
) -> Result<UnsafeIntrusiveEntityRef<T>, Report> {
    let source_manager = &config.context.session().source_manager;
    let source_file = source_manager.load_file(path.as_ref()).map_err(Report::msg)?;
    parse_source(config, source_file)
}

/// Parse any operation from `source` with the provided `uri` and `config`
pub fn parse_any(
    config: ParserConfig,
    uri: Uri,
    source: impl Into<String>,
) -> Result<OperationRef, Report> {
    use midenc_session::diagnostics::SourceLanguage;
    let source_manager = &config.context.session().source_manager;
    let source_file = source_manager.load(SourceLanguage::Other("hir"), uri, source.into());
    parse_anchored_source(None, config, source_file)
}

/// Parse IR assembly anchored at an operation `name`, from `source` with the provided `uri` and `config`
pub fn parse_anchored(
    name: OperationName,
    config: ParserConfig,
    uri: Uri,
    source: impl Into<String>,
) -> Result<OperationRef, Report> {
    use midenc_session::diagnostics::SourceLanguage;
    let source_manager = &config.context.session().source_manager;
    let source_file = source_manager.load(SourceLanguage::Other("hir"), uri, source.into());
    parse_anchored_source(Some(name), config, source_file)
}

/// Parse any operation from `path` with the provided `config`
#[cfg(feature = "std")]
pub fn parse_file_any(
    config: ParserConfig,
    path: impl AsRef<std::path::Path>,
) -> Result<OperationRef, Report> {
    let source_manager = &config.context.session().source_manager;
    let source_file = source_manager.load_file(path.as_ref()).map_err(Report::msg)?;
    parse_anchored_source(None, config, source_file)
}

/// Parse IR assembly anchored at an operation `name`, from `path` with the provided `config`
#[cfg(feature = "std")]
pub fn parse_file_anchored(
    anchor: OperationName,
    config: ParserConfig,
    path: impl AsRef<std::path::Path>,
) -> Result<OperationRef, Report> {
    let source_manager = &config.context.session().source_manager;
    let source_file = source_manager.load_file(path.as_ref()).map_err(Report::msg)?;
    parse_anchored_source(Some(anchor), config, source_file)
}

fn parse_anchored_source(
    anchor: Option<OperationName>,
    config: ParserConfig,
    source_file: Arc<SourceFile>,
) -> Result<OperationRef, Report> {
    use crate::{BuilderExt, dialects::builtin::World};

    let source = source_file.as_str();
    let scanner = Scanner::new(source);
    let token_stream = TokenStream::new(source_file.id(), scanner);
    let mut parser = DefaultParser::new(ParserState::new(config, token_stream));
    let span = parser.current_location();
    let world = parser.builder_mut().create::<World, ()>(span)()?;
    let mut asm_state = Box::<AsmParserState>::default();
    {
        asm_state.initialize(world.as_operation_ref());
        parser.state_mut().asm_state = Some(asm_state);
    }
    let mut operation_parser = operation::OperationParser::new(parser, world);
    let op = operation_parser
        .parse_operation()
        .map_err(|err| Report::from(err).with_source_code(source_file.clone()))?;
    operation_parser
        .finalize()
        .map_err(|err| Report::from(err).with_source_code(source_file.clone()))?;

    match anchor {
        None => Ok(op),
        Some(anchor) => {
            let operation = op.borrow();
            if operation.name() != anchor {
                return Err(Report::msg(format!(
                    "expected operation '{anchor}', got '{}'",
                    operation.name()
                )));
            }

            Ok(op)
        }
    }
}

fn parse_source<T: OpParser + OpRegistration>(
    config: ParserConfig,
    source_file: Arc<SourceFile>,
) -> Result<UnsafeIntrusiveEntityRef<T>, Report> {
    let name = config.context.get_registered_name::<T>();
    let op = parse_anchored_source(Some(name), config, source_file)?;
    let op = op.borrow();
    Ok(unsafe { UnsafeIntrusiveEntityRef::from_raw(op.downcast_ref::<T>().unwrap()) })
}

/// This trait is implemented by parsers which can parse operations from IR assembly, and provides
/// higher-level parser functions for that purpose.
pub trait OpAsmParser<'input>: Parser<'input> {
    /// Parse a `loc(...)` specifier if present.
    ///
    /// Location for BlockArgument and Operation may be deferred with an alias, in
    /// which case an OpaqueLoc is set and will be resolved when parsing
    /// completes.
    fn parse_optional_location_specifier(&mut self) -> ParseResult<Option<Location>>;

    /// Return the name of the specified result in the specified syntax, as well
    /// as the sub-element in the name.  It returns an empty string and ~0U for
    /// invalid result numbers.  For example, in this operation:
    ///
    ///  %x, %y:2, %z = foo.op
    ///
    ///    getResultName(0) == {"x", 0 }
    ///    getResultName(1) == {"y", 0 }
    ///    getResultName(2) == {"y", 1 }
    ///    getResultName(3) == {"z", 0 }
    ///    getResultName(4) == {"", ~0U }
    fn get_result_name(&self, result_num: u8) -> Option<(interner::Symbol, u8)>;

    /// Returns the number of declared SSA results.
    fn get_num_results(&self) -> usize;

    /// Parse an operation in its generic form.
    ///
    /// The parsed operation is parsed in the current context, and inserted in the provided block
    /// and insertion point. The results produced by this operation aren't mapped to any named value
    /// in the parser.
    fn parse_generic_operation(&mut self, ip: Option<ProgramPoint>) -> ParseResult<OperationRef>;

    /// Parse the name of an operation, in the custom form.
    fn parse_custom_operation_name(&mut self) -> ParseResult<Span<OperationName>>;

    // /// Parse the name of an operation.
    //fn parse_operation_name(&mut self) -> ParseResult<OperationName>;

    /// Parse different components, e.g. operand, successors, regions, attribute and function
    /// signature - of the generic form of an operation instance and populate the input
    /// [OperationState] 'result' with those components.
    ///
    /// If any of the components is explicitly provided, then skip parsing that component.
    fn parse_generic_operation_after_name(
        &mut self,
        state: &mut OperationState,
        operands: Option<&[UnresolvedOperand]>,
        successors: Option<&[BlockRef]>,
        regions: Option<&[RegionRef]>,
        attrs: Option<ParsedAttrs>,
        signature: Option<FunctionType>,
    ) -> ParseResult;

    /// Parse a single SSA value operand name.
    fn parse_operand(&mut self, allow_result_number: bool) -> ParseResult<UnresolvedOperand>;

    /// Parse a single SSA value operand name, if present.
    fn parse_optional_operand(
        &mut self,
        allow_result_number: bool,
    ) -> ParseResult<Option<UnresolvedOperand>>;

    /// Parse zero or more comma-separated SSA operand references with a specified delimiter, and
    /// an optional required operand count.
    fn parse_operand_list(
        &mut self,
        result: &mut SmallVec<[UnresolvedOperand; 2]>,
        delimiter: Delimiter,
        allow_result_number: bool,
        required_operand_count: Option<NonZeroU8>,
    ) -> ParseResult {
        self.parse_comma_separated_list(delimiter, Some("operand list"), |parser| {
            let operand = parser.parse_operand(allow_result_number)?;
            result.push(operand);

            Ok(true)
        })?;

        if let Some(required) = required_operand_count
            && result.len() != required.get() as usize
        {
            todo!()
        }

        Ok(())
    }

    /// Parse zero or more trailing SSA comma-separated trailing operand references with a specified
    /// surrounding delimiter, and an optional required operand count.
    ///
    /// A leading comma is expected before the operands.
    fn parse_trailing_operand_list(
        &mut self,
        result: &mut SmallVec<[UnresolvedOperand; 2]>,
        delimiter: Delimiter,
    ) -> ParseResult {
        if self.parse_optional_comma()? {
            self.parse_operand_list(result, delimiter, true, None)
        } else {
            Ok(())
        }
    }

    /// Resolve an operand to an SSA value, emitting an error on failure.
    fn resolve_operand(&mut self, operand: UnresolvedOperand, ty: Type) -> ParseResult<ValueRef>;

    /// Resolve an operand to an SSA value, emitting an error on failure.
    fn resolve_operands_of_uniform_type(
        &mut self,
        operands: &[UnresolvedOperand],
        ty: &Type,
        result: &mut SmallVec<[ValueRef; 2]>,
    ) -> ParseResult {
        for operand in operands.iter().copied() {
            result.push(self.resolve_operand(operand, ty.clone())?);
        }
        Ok(())
    }

    /// Resolve an operand to an SSA value, emitting an error on failure.
    fn resolve_operands(
        &mut self,
        span: SourceSpan,
        operands: &[UnresolvedOperand],
        tys: &[Type],
        result: &mut SmallVec<[ValueRef; 2]>,
    ) -> ParseResult {
        if operands.len() != tys.len() {
            return Err(ParserError::OperandAndTypeListMismatch {
                span,
                num_operands: operands.len(),
                num_types: tys.len(),
            });
        }
        for (operand, ty) in operands.iter().copied().zip(tys.iter().cloned()) {
            result.push(self.resolve_operand(operand, ty)?);
        }
        Ok(())
    }

    /// Parse a single argument with the following syntax:
    ///
    ///   `%ssaName : !type { optionalAttrDict} loc(optionalSourceLoc)`
    ///
    /// If `allow_type` is false or `allow_attrs` are false then the respective parts of the grammar
    /// are not parsed.
    fn parse_argument(&mut self, allow_type: bool, allow_attrs: bool) -> ParseResult<Argument>;

    /// Parse a single argument, if present, with the following syntax:
    ///
    ///   `%ssaName : !type { optionalAttrDict} loc(optionalSourceLoc)`
    ///
    /// If `allow_type` is false or `allow_attrs` are false then the respective parts of the grammar
    /// are not parsed.
    fn parse_optional_argument(
        &mut self,
        allow_type: bool,
        allow_attrs: bool,
    ) -> ParseResult<Option<Argument>>;

    /// Parse zero or more arguments with a specified surrounding delimiter.
    fn parse_argument_list(
        &mut self,
        delimiter: Delimiter,
        allow_type: bool,
        allow_attrs: bool,
        result: &mut SmallVec<[Argument; 4]>,
    ) -> ParseResult {
        self.parse_comma_separated_list(delimiter, Some("argument list"), |parser| {
            let arg = parser.parse_optional_argument(allow_type, allow_attrs)?;
            if let Some(arg) = arg {
                result.push(arg);
                Ok(true)
            } else {
                Ok(false)
            }
        })
    }

    /// Parses a region. Any parsed blocks are appended to 'region' and must be moved to the op
    /// regions after the op is created. The first block of the region takes 'arguments'.
    ///
    /// If 'enable_name_shadowing' is set to true, the argument names are allowed to shadow the
    /// names of other existing SSA values defined above the region scope. 'enable_name_shadowing'
    /// can only be set to true for regions attached to operations that are 'IsolatedFromAbove'.
    fn parse_region(
        &mut self,
        region: RegionRef,
        arguments: &[Argument],
        enable_name_shadowing: bool,
    ) -> ParseResult;

    /// Parses a region if present.
    fn parse_optional_region(
        &mut self,
        arguments: &[Argument],
        enable_name_shadowing: bool,
    ) -> ParseResult<Option<RegionRef>>;

    /// Parses a region preceded by the given keyword token, if the keyword is the next token
    fn parse_optional_region_with_token(
        &mut self,
        keyword: &str,
        arguments: &[Argument],
        enable_name_shadowing: bool,
    ) -> ParseResult<Option<RegionRef>> {
        if self.token_stream_mut().next_if_eq(Token::BareIdent(keyword))? {
            let region = self.context().create_region();
            self.parse_region(region, arguments, enable_name_shadowing)?;
            Ok(Some(region))
        } else {
            Ok(None)
        }
    }

    /// Parse a single operation successor.
    fn parse_successor(&mut self) -> ParseResult<Span<BlockRef>>;

    /// Parse an optional operation successor.
    fn parse_optional_successor(&mut self) -> ParseResult<Option<Span<BlockRef>>>;

    /// Parse a single operation successor and its operand list.
    fn parse_successor_and_use_list(
        &mut self,
        operands: &mut SmallVec<[ValueRef; 2]>,
    ) -> ParseResult<Span<BlockRef>>;
}

/// This trait is implemented by dialects which hook into IR assembly parsing in specific ways.
///
/// NOTE: This is currently an empty trait until we add support for dialect parsing hooks.
pub trait OpAsmDialectInterface: crate::Dialect {}

/// This trait is implemented by operations which hook into IR assembly parsing in specific ways.
pub trait OpAsmOpInterface: crate::Op {
    /// Get the special names to use when printing the results of this operation.
    ///
    /// Each result value in the returned vector starts a result "pack" starting at that result,
    /// giving the name to that pack. To signal that a result pack should use the default naming
    /// scheme, `None` can be provided instead of a name.
    ///
    /// For example, if you have an operation that has four results and you want to split these into
    /// three distinct groups you could do the following:
    ///
    /// ```text
    /// let results = self.results().all();
    /// smallvec::smallvec![
    ///     (results[0], Some("first_result".into()),
    ///     (results[1], Some("middle_results".into()),
    ///     (results[3], None /* use the default numbering */)
    /// ]
    /// ```
    ///
    /// This would print the operation as follows:
    ///
    /// ```hir
    /// %first_result, %middle_results:2, %0 = "my.op" ...
    /// ```
    fn get_asm_result_names(&self) -> SmallVec<[(OpResultRef, Option<CompactString>); 2]> {
        SmallVec::new_const()
    }
    /// Get the names to use when printing the block arguments for a region immediately nested
    /// under this operation.
    ///
    /// If no entry is present in the returned map for a given argument, then the default is used
    ///
    /// The default implementation returns an empty map
    fn get_asm_block_argument_names(
        &self,
        region: RegionRef,
    ) -> SmallOrdMap<BlockArgumentRef, CompactString, 4> {
        SmallOrdMap::new()
    }
    /// Get the names to use for the each block inside a region attached to this operation.
    ///
    /// If no entry is present in the returned map for a given block, then the default name is used
    ///
    /// The default implementation returns an empty map
    fn get_asm_block_names(&self) -> SmallOrdMap<BlockRef, CompactString, 1> {
        SmallOrdMap::new()
    }
    /// Return the default dialect used when printing/parsing operations in regions nested under
    /// this operation.
    ///
    /// This allows for eliding the dialect prefix from the operation name, for example it would be
    /// possible to omit the `scf.` prefix from all operations within a `scf.while`  if this method
    /// returned `scf`.
    ///
    /// The default implementation returns `None`.
    fn get_default_dialect(&self) -> Option<interner::Symbol> {
        None
    }
}

/// Represents a value parsed from an argument list, with an associated type and attributes.
///
/// This is a richer version of [UnresolvedOperand], as most operands cannot have associated
/// attributes.
#[derive(Debug)]
pub struct Argument {
    pub name: UnresolvedOperand,
    pub ty: Type,
    pub attrs: ParsedAttrs,
    pub loc: Location,
}

impl Argument {
    pub fn has_attribute(&self, name: impl Into<interner::Symbol>) -> bool {
        let name = name.into();
        self.attrs.iter().any(|attr| attr.name == name)
    }
}

/// Represents a value parsed from IR assembly that has not been resolved to a concrete [ValueRef]
/// yet.
#[derive(Copy, Clone)]
pub struct UnresolvedOperand {
    pub loc: SourceSpan,
    pub name: ValueId,
}

impl core::fmt::Debug for UnresolvedOperand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UnresolvedOperand")
            .field_with("name", |f| write!(f, "{}", &self.name))
            .finish()
    }
}

/// A type alias for a vector of [NamedAttribute]
pub type ParsedAttrs = SmallVec<[NamedAttribute; 1]>;
