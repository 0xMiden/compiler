use super::*;
use crate::{
    BlockArgument, Builder, EntityWithId, Forward, FunctionType, FxHashSet, InsertionGuard, Op,
    PendingSuccessorInfo, RawWalk, Value, WalkResult,
    adt::{SmallDenseMap, SmallSet},
    dialects::builtin::{
        UnrealizedConversionCast, WorldRef,
        attributes::{Location, LocationAttr},
    },
    traits::{IsolatedFromAbove, Terminator},
};

/// This class provides support for parsing operations and regions of operations.
pub struct OperationParser<P> {
    parser: P,
    /// The top level operation that holds all of the parsed operations.
    top_level: WorldRef,

    /// A list of isolated name scopes.
    isolated_name_scopes: SmallVec<[IsolatedSSANameScope; 2]>,

    /// This keeps track of the block names as well as the location of the first reference for each
    /// nested name scope.
    ///
    /// This is used to diagnose invalid block references and memorize them.
    blocks_by_name: SmallVec<[SmallDenseMap<BlockId, Span<BlockRef>>; 2]>,
    forward_ref: SmallVec<[SmallDenseMap<BlockRef, SourceSpan>; 2]>,

    /// These are all of the placeholders we've made along with the location of their first
    /// reference, to allow checking for use of undefined values.
    forward_ref_placeholders: SmallDenseMap<ValueRef, SourceSpan>,

    /// Operations that define the placeholders.
    ///
    /// These are kept until the end of of the lifetime of the parser because some custom parsers
    /// may store references to them in local state and use them after forward references
    /// have been resolved.
    forward_ref_ops: SmallSet<OperationRef, 2>,

    /// Deferred locations: when parsing `loc(#loc42)` we add an entry to this map.
    ///
    /// After parsing the definition `#loc42 = ...` we'll patch back users of this location.
    deferred_locs_references: Vec<DeferredLocInfo>,
}

impl<'input, P> Parser<'input> for OperationParser<P>
where
    P: Parser<'input>,
{
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        self.parser.builder()
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        self.parser.builder_mut()
    }

    #[inline(always)]
    fn state(&self) -> &ParserState<'input> {
        self.parser.state()
    }

    #[inline(always)]
    fn state_mut(&mut self) -> &mut ParserState<'input> {
        self.parser.state_mut()
    }

    #[inline(always)]
    fn token_stream(&self) -> &TokenStream<'input> {
        self.parser.token_stream()
    }

    #[inline(always)]
    fn token_stream_mut(&mut self) -> &mut TokenStream<'input> {
        self.parser.token_stream_mut()
    }

    #[inline]
    fn parse_extended_attribute(&mut self, ty: &Type) -> ParseResult<Span<AttributeRef>> {
        super::parser::parse_extended_attribute(self, ty)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct DeferredLocInfo {
    loc: SourceSpan,
    identifier: interner::Symbol,
}

/// This type is used to keep track of things that are either an Operation or a BlockArgument.
///
/// We cannot use Value for this, because not all Operations have results.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OpOrArgument {
    Op(OperationRef),
    Arg(BlockArgumentRef),
}

impl OpOrArgument {
    pub fn span(&self) -> SourceSpan {
        use crate::diagnostics::Spanned;
        match self {
            Self::Op(op) => op.borrow().span,
            Self::Arg(arg) => arg.borrow().span(),
        }
    }

    pub fn set_span(self, span: SourceSpan) {
        match self {
            Self::Op(mut op) => op.borrow_mut().set_span(span),
            Self::Arg(mut arg) => arg.borrow_mut().set_span(span),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ResultRecord {
    pub loc: SourceSpan,
    pub id: interner::Symbol,
    pub count: u8,
}

/// This struct represents an isolated SSA name scope.
///
/// This scope may contain other nested non-isolated scopes. These scopes are used for operations
/// that are known to be isolated to allow for reusing names within their regions, even if those
/// names are used above.
#[derive(Default)]
struct IsolatedSSANameScope {
    /// This keeps track of all of the SSA values we are tracking for each name scope, indexed by
    /// their name.
    ///
    /// This has one entry per result number.
    values: FxHashMap<ValueId, SmallVec<[Option<Span<ValueRef>>; 1]>>,
    /// This keeps track of all of the values defined by a specific name scope.
    definitions_per_scope: SmallVec<[FxHashSet<ValueId>; 2]>,
}

impl IsolatedSSANameScope {
    /// Record that a definition was added at the current scope.
    pub fn record_definition(&mut self, def: ValueId) {
        self.definitions_per_scope.last_mut().unwrap().insert(def);
    }

    /// Push a nested name scope.
    pub fn push_ssa_name_scope(&mut self) {
        self.definitions_per_scope.push(Default::default());
    }

    /// Pop a nested name scope.
    pub fn pop_ssa_name_scope(&mut self) {
        for def in self.definitions_per_scope.pop().unwrap() {
            self.values.remove(&def);
        }
    }
}

impl<'input, P> OperationParser<P>
where
    P: Parser<'input>,
{
    pub fn new(mut parser: P, top_level: WorldRef) -> Self {
        // Ensure the world's body region is populated
        {
            let world_body = { top_level.borrow().body().as_region_ref() };
            if world_body.borrow().is_empty() {
                parser.builder_mut().create_block(world_body, None, &[]);
            } else {
                parser
                    .builder_mut()
                    .set_insertion_point_to_end(world_body.borrow().entry_block_ref().unwrap());
            }
        }

        let mut this = Self {
            parser,
            top_level,
            isolated_name_scopes: Default::default(),
            blocks_by_name: Default::default(),
            forward_ref: Default::default(),
            forward_ref_placeholders: Default::default(),
            forward_ref_ops: Default::default(),
            deferred_locs_references: Default::default(),
        };

        // The top level operation starts a new name scope.
        this.push_ssa_name_scope(true);

        // If we are populating the parser state, prepare it for parsing.
        if let Some(state) = this.parser.state_mut().asm_state.as_deref_mut() {
            state.initialize(top_level.as_operation_ref());
        }

        this
    }

    /// After parsing is finished, this function must be called to see if there are any remaining
    /// issues.
    pub fn finalize(mut self) -> ParseResult {
        // Check for any forward references that are left.  If we find any, error out.
        if !self.forward_ref_placeholders.is_empty() {
            let mut labels = Vec::with_capacity(self.forward_ref_placeholders.len());
            for (_, span) in self.forward_ref_placeholders.iter() {
                labels.push(LabeledSpan::new_with_span(None, *span));
            }
            return Err(ParserError::UndeclaredValueUses { labels });
        }

        // Resolve the locations of any deferred operations.
        let attribute_aliases = &self.parser.state().symbols.attribute_alias_definitions;
        let resolve_location = |op_or_argument: OpOrArgument| {
            let fwd_loc = op_or_argument.span();
            let Some(loc_index) = Location::is_deferred(fwd_loc) else {
                return Ok(());
            };
            let loc_info = self.deferred_locs_references[loc_index];
            let Some(attr) = attribute_aliases.get(&loc_info.identifier) else {
                return Err(ParserError::UnresolvedLocationAlias { span: loc_info.loc });
            };
            let Some(loc_attr) = attr.try_downcast_attr::<LocationAttr>().ok() else {
                return Err(ParserError::InvalidLocationAlias {
                    span: loc_info.loc,
                    reason: format!("expected location, but found '{:?}'", &attr.borrow()),
                });
            };
            let loc = loc_attr.borrow().as_value().try_into_span(self.context());
            op_or_argument.set_span(loc.unwrap_or(SourceSpan::UNKNOWN));
            Ok(())
        };

        let walk_result =
            self.top_level
                .as_operation_ref()
                .raw_prewalk::<Forward, _, _>(|op: OperationRef| {
                    if let Err(err) = resolve_location(OpOrArgument::Op(op)) {
                        return WalkResult::Break(err);
                    }
                    let op = op.borrow();
                    for region in op.regions() {
                        for block in region.body() {
                            for arg in block.arguments() {
                                if let Err(err) = resolve_location(OpOrArgument::Arg(*arg)) {
                                    return WalkResult::Break(err);
                                }
                            }
                        }
                    }
                    WalkResult::Continue(())
                });

        if let WalkResult::Break(err) = walk_result {
            return Err(err);
        }

        // Pop the top level name scope.
        self.pop_ssa_name_scope()?;

        // Verify that the parsed operations are valid.
        if self.parser.state().config.should_verify_after_parse() {
            self.top_level.borrow().as_operation().recursively_verify()?;
        }

        // If we are populating the parser state, finalize the top-level operation.
        if let Some(asm_state) = self.parser.state_mut().asm_state.as_deref_mut() {
            asm_state.finalize(self.top_level.as_operation_ref());
        }

        Ok(())
    }
}

/// SSA Value Handling
impl<'input, P> OperationParser<P>
where
    P: Parser<'input>,
{
    /// Push a new SSA name scope to the parser.
    fn push_ssa_name_scope(&mut self, isolated: bool) {
        self.blocks_by_name.push(Default::default());
        self.forward_ref.push(Default::default());

        // Push back a new name definition scope.
        if isolated {
            self.isolated_name_scopes.push(Default::default());
        }
        self.isolated_name_scopes.last_mut().unwrap().push_ssa_name_scope();
    }

    /// Pop the last SSA name scope from the parser.
    fn pop_ssa_name_scope(&mut self) -> ParseResult {
        let forward_ref_current_scope = self.forward_ref.pop().unwrap();

        // Verify that all referenced blocks were defined.
        if !forward_ref_current_scope.is_empty() {
            let mut labels = Vec::default();
            for (block, span) in forward_ref_current_scope {
                labels.push(LabeledSpan::new_with_span(None, span));
                // Add this block to the top-level region to allow for automatic cleanup.
                self.top_level.borrow_mut().body_mut().body_mut().push_back(block);
            }
            return Err(ParserError::UndefinedBlocks { labels });
        }

        // Pop the next nested namescope. If there is only one internal namescope, just pop the
        // isolated scope.
        let current_name_scope = self.isolated_name_scopes.last_mut().unwrap();
        if current_name_scope.definitions_per_scope.len() == 1 {
            self.isolated_name_scopes.pop();
        } else {
            current_name_scope.pop_ssa_name_scope();
        }

        self.blocks_by_name.pop();

        Ok(())
    }

    /// Register a definition of a value with the symbol table.
    fn add_definition(&mut self, use_info: UnresolvedOperand, value: ValueRef) -> ParseResult {
        let entries = self.get_ssa_value_entry(use_info.name);

        // Make sure there is a slot for this value.
        let result_index = use_info.name.result_index().unwrap_or(0) as usize;
        if entries.len() <= result_index {
            entries.resize(result_index + 1, None);
        }

        // If we already have an entry for this, check to see if it was a definition or a forward
        // reference.
        if let Some(mut existing) = entries[result_index] {
            if !self.forward_ref_placeholders.contains_key(existing.inner()) {
                return Err(ParserError::ValueRedefinition {
                    span: use_info.loc,
                    prev_span: existing.span(),
                });
            }

            if existing.borrow().ty() != value.borrow().ty() {
                return Err(ParserError::ValueDefinitionTypeMismatch {
                    span: use_info.loc,
                    prev_span: existing.span(),
                    ty: value.borrow().ty().clone(),
                    prev_ty: existing.borrow().ty().clone(),
                });
            }

            // If it was a forward reference, update everything that used it to use the actual
            // definition instead, delete the forward ref, and remove it from our set of forward
            // references we track.
            existing.borrow_mut().replace_all_uses_with(value);
            self.forward_ref_placeholders.remove(&existing);

            // If a definition of the value already exists, replace it in the assembly
            // state.
            if let Some(asm_state) = self.parser.state_mut().asm_state.as_deref_mut() {
                asm_state.refine_definition(existing.into_inner(), value);
            }
        }

        // Record this definition for the current scope.
        let entries = self.get_ssa_value_entry(use_info.name);
        entries[result_index] = Some(Span::new(use_info.loc, value));
        self.record_definition(use_info.name);

        Ok(())
    }

    /// Parse an optional list of SSA uses into 'results'.
    ///
    ///   ssa-use-list ::= ssa-use (`,` ssa-use)*
    ///   ssa-use-list-opt ::= ssa-use-list?
    ///
    fn parse_optional_ssa_use_list<const N: usize>(
        &mut self,
        results: &mut SmallVec<[UnresolvedOperand; N]>,
    ) -> ParseResult {
        if !self
            .parser
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::PercentIdent(_)))
        {
            return Ok(());
        }

        self.parse_comma_separated_list(Delimiter::None, Some("SSA use list"), |parser| {
            let result = parser.parse_ssa_use(/*allow_result_number*/ true)?;
            results.push(result);
            Ok(true)
        })
    }

    /// Parse a single SSA use into 'result'.
    ///
    /// If 'allow_result_number' is true then we allow `#42` syntax.
    ///
    ///   ssa-use ::= ssa-id
    ///
    fn parse_ssa_use(&mut self, allow_result_number: bool) -> ParseResult<UnresolvedOperand> {
        let (span, id) = self
            .parser
            .token_stream_mut()
            .expect_map("SSA value", |tok| match tok {
                Token::PercentIdent(id) => Some(ValueId::from_symbol(interner::Symbol::intern(id))),
                _ => None,
            })?
            .into_parts();

        // If we have an attribute ID, it is a result number.
        let result_num = self.parser.token_stream_mut().next_if_map(|tok| match tok {
            Token::HashIdent(num) => Some(num),
            _ => None,
        })?;
        if let Some(result_num) = result_num {
            let (result_num_span, result_num) = result_num.into_parts();
            if !allow_result_number {
                return Err(ParserError::ResultNumberUsedInArgumentList {
                    span: result_num_span,
                    value_span: span,
                });
            }
            let index =
                result_num.parse::<u8>().map_err(|err| ParserError::InvalidResultIndex {
                    span: result_num_span,
                    value_span: span,
                    reason: err.to_string(),
                })?;

            Ok(UnresolvedOperand {
                loc: span,
                name: id.with_result_index(index),
            })
        } else {
            Ok(UnresolvedOperand {
                loc: span,
                name: id,
            })
        }
    }

    /// Given a reference to an SSA value and its type, return a reference.
    ///
    /// This returns `None` on failure.
    fn resolve_ssa_use(&mut self, use_info: UnresolvedOperand, ty: Type) -> ParseResult<ValueRef> {
        let entries = self.get_ssa_value_entry(use_info.name);

        // If we have already seen a value of this name, return it.
        let result_index = use_info.name.result_index().unwrap_or(0) as usize;
        if result_index < entries.len()
            && let Some(value_ref) = entries[result_index]
        {
            // Check that the type matches the other uses.
            let (span, value_ref) = value_ref.into_parts();
            let value = value_ref.borrow();
            let prev_ty = value.ty();
            if prev_ty == &ty || matches!(ty, Type::Unknown) {
                if let Some(asm_state) = self.parser.state_mut().asm_state.as_deref_mut() {
                    asm_state.add_uses(value_ref, &[use_info.loc]);
                }
                return Ok(value_ref);
            }

            return Err(ParserError::ValueUseTypeMismatch {
                span: use_info.loc,
                prev_span: span,
                ty,
                prev_ty: prev_ty.clone(),
            });
        }

        // Make sure we have enough slots for this.
        if entries.len() <= result_index {
            entries.resize(result_index + 1, None);
        }

        // If the value has already been defined and this is an overly large result number,
        // diagnose that.
        if entries[0].is_some_and(|v| !self.is_forward_ref_placeholder(v.into_inner())) {
            return Err(ParserError::InvalidResultIndex {
                span: use_info.loc,
                value_span: SourceSpan::UNKNOWN,
                reason: format!("{} has index {result_index}", &use_info.name),
            });
        }

        // Otherwise, this is a forward reference.
        //
        // Create a placeholder and remember that we did so.
        let result = self.create_forward_ref_placeholder(use_info.loc, ty);
        let entries = self.get_ssa_value_entry(use_info.name);
        entries[result_index] = Some(Span::new(use_info.loc, result));

        if let Some(asm_state) = self.parser.state_mut().asm_state.as_deref_mut() {
            asm_state.add_uses(result, &[use_info.loc]);
        }
        Ok(result)
    }

    /// Parse an SSA use with an associated type.
    ///
    ///   ssa-use-and-type ::= ssa-use `:` type
    fn parse_ssa_def_or_use_and_type<F>(&mut self, mut action: F) -> ParseResult
    where
        F: FnMut(&mut Self, UnresolvedOperand, Type) -> ParseResult,
    {
        let use_info = self.parse_ssa_use(true)?;
        let ty = self.parser.parse_colon_type()?;

        action(self, use_info, ty.into_inner())
    }

    /// Parse a (possibly empty) list of SSA operands, followed by a colon, then
    /// followed by a type list.
    ///
    ///   ssa-use-and-type-list ::= ssa-use-list ':' type-list-no-parens
    ///
    fn parse_optional_ssa_use_and_type_list<const N: usize>(
        &mut self,
        results: &mut SmallVec<[ValueRef; N]>,
    ) -> ParseResult {
        let mut value_ids = SmallVec::<[UnresolvedOperand; 4]>::new_const();
        self.parse_optional_ssa_use_list(&mut value_ids)?;

        // If there were no operands, then there is no colon or type lists.
        if value_ids.is_empty() {
            return Ok(());
        }

        let mut types = SmallVec::<[Type; 4]>::new_const();
        self.parser.token_stream_mut().expect(Token::Comma)?;
        self.parser.parse_type_list_no_parens(&mut types)?;

        if value_ids.len() != types.len() {
            let start = value_ids[0].loc;
            let end = self.parser.token_stream().current_position();
            return Err(ParserError::MismatchedValueAndTypeLists {
                span: SourceSpan::new(start.source_id(), start.start()..end),
                num_values: value_ids.len(),
                num_types: types.len(),
            });
        }

        results.reserve(value_ids.len());
        for (unresolved, ty) in value_ids.into_iter().zip(types) {
            results.push(self.resolve_ssa_use(unresolved, ty)?);
        }

        Ok(())
    }

    /// Return the location of the value identified by its name and number if it has been already
    /// referenced.
    fn get_reference_loc(&mut self, id: ValueId) -> Option<SourceSpan> {
        let values = &self.isolated_name_scopes.last().unwrap().values;
        let entry = values.get(&id.without_result_index())?;
        let result_index = id.result_index().unwrap_or(0) as usize;
        entry.get(result_index).and_then(|v| v.map(|v| v.span()))
    }

    /// Record that a definition was added at the current scope.
    fn record_definition(&mut self, id: ValueId) {
        self.isolated_name_scopes.last_mut().unwrap().record_definition(id);
    }

    /// Get the value entry for the given SSA name.
    fn get_ssa_value_entry(&mut self, id: ValueId) -> &mut SmallVec<[Option<Span<ValueRef>>; 1]> {
        self.isolated_name_scopes
            .last_mut()
            .unwrap()
            .values
            .entry(id.without_result_index())
            .or_default()
    }

    /// Create and remember a new placeholder for a forward reference.
    fn create_forward_ref_placeholder(&mut self, loc: SourceSpan, ty: Type) -> ValueRef {
        // Forward references are always created as operations, because we just need something with
        // a def/use chain.
        //
        // We create these placeholders as having an empty name, which we know cannot be created
        // through normal user input, allowing us to distinguish them.
        let name = self.parser.context().get_registered_name::<UnrealizedConversionCast>();
        // We create by hand here, as we're creating an op that expects an operand without one,
        // if this turns out to be a problem, we may need to create a dedicated op for this.
        let mut op_ref = UnrealizedConversionCast::alloc_default(self.parser.context_rc());
        let result = {
            let mut op = op_ref.borrow_mut();
            op.set_span(loc);
            op.set_ty(ty.clone());
            let mut op = op.as_operation_mut();
            let result = op.context().make_result(loc, ty, op.as_operation_ref(), 0);
            op.results_mut().group_mut(0).push(result);
            result as ValueRef
        };
        self.forward_ref_placeholders.insert(result, loc);
        self.forward_ref_ops.insert(op_ref.as_operation_ref());
        result
    }
}

/// Operation Parsing
impl<'input, P> OperationParser<P>
where
    P: Parser<'input>,
{
    /// Parse an operation.
    ///
    ///  operation         ::= op-result-list?
    ///                        (generic-operation | custom-operation)
    ///                        trailing-location?
    ///  generic-operation ::= string-literal `(` ssa-use-list? `)`
    ///                        successor-list? (`(` region-list `)`)?
    ///                        attribute-dict? `:` function-type
    ///  custom-operation  ::= bare-id custom-operation-format
    ///  op-result-list    ::= op-result (`,` op-result)* `=`
    ///  op-result         ::= ssa-id (`:` integer-literal)
    ///
    pub fn parse_operation(&mut self) -> ParseResult<OperationRef> {
        let start = self.parser.token_stream().current_position();

        let mut result_ids = SmallVec::<[ResultRecord; 1]>::new_const();
        let mut num_expected_results = 0;
        if self
            .parser
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::PercentIdent(_)))
        {
            // Parse the group of result ids.
            self.parse_comma_separated_list(Delimiter::None, Some("ssa identifiers"), |parser| {
                // Parse the next result id.
                let (name_span, name) = parser
                    .token_stream_mut()
                    .expect_map("SSA identifier", |tok| match tok {
                        Token::PercentIdent(id) => Some(id),
                        _ => None,
                    })?
                    .into_parts();

                // If the next token is a ':', we parse the expected result count.
                let mut expected_sub_results = 1;
                let mut end = name_span.end();
                if parser.token_stream_mut().next_if_eq(Token::Colon)? {
                    // Check that the next token is an integer.
                    let (count_span, count_str) = parser
                        .token_stream_mut()
                        .expect_map("integer number of results", |tok| match tok {
                            Token::Int(n) => Some(n),
                            _ => None,
                        })?
                        .into_parts();
                    let count = count_str.parse::<u8>().map_err(|err| {
                        ParserError::InvalidIntegerLiteral {
                            span: count_span,
                            reason: err.to_string(),
                        }
                    })?;
                    if count == 0 {
                        return Err(ParserError::InvalidResultCount { span: count_span });
                    }
                    end = count_span.end();
                    expected_sub_results = count;
                }

                let span = SourceSpan::new(name_span.source_id(), name_span.start()..end);
                result_ids.push(ResultRecord {
                    loc: span,
                    id: interner::Symbol::intern(name),
                    count: expected_sub_results,
                });
                num_expected_results += expected_sub_results;
                Ok(true)
            })?;

            self.parser.token_stream_mut().expect(Token::Equal)?;
        }

        let source_id = self.source_id();
        let Some(name_token) = self
            .parser
            .token_stream_mut()
            .peek()?
            .map(|(start, tok, end)| spanned!(source_id, start, end, tok))
        else {
            return Err(ParserError::UnexpectedEof {
                expected: vec!["operation name".to_string()],
            });
        };

        let (name_span, name_token) = name_token.into_parts();
        let op = match name_token {
            Token::BareIdent(_) => self.parse_custom_operation(&result_ids)?,
            Token::String(_) => self.parse_generic_operation(None)?,
            invalid => {
                return Err(ParserError::UnexpectedToken {
                    span: name_span,
                    token: invalid.to_string(),
                    expected: Some("operation name".to_string()),
                });
            }
        };

        // If the operation had a name, register it.
        let end = self.current_location();
        if !result_ids.is_empty() {
            let op = op.borrow();
            match op.num_results() {
                0 => return Err(ParserError::NamedOpWithNoResults { span: name_span }),
                n if n != num_expected_results as usize => {
                    return Err(ParserError::ResultCountMismatch {
                        span: name_span,
                        count: n,
                        expected: num_expected_results,
                    });
                }
                _ => (),
            }

            // Add this operation to the assembly state if it was provided to populate.
            if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
                let mut asm_result_groups = SmallVec::<[_; 4]>::new_const();
                asm_result_groups.reserve(result_ids.len());
                let mut result_index = 0;
                for record in result_ids.iter() {
                    asm_result_groups.push((result_index as usize, record.loc));
                    result_index += record.count;
                }
                asm_state.finalize_operation_definition(
                    op.as_operation_ref(),
                    name_span,
                    end,
                    &asm_result_groups,
                );
            }

            // Add definitions for each of the result groups.
            for (result_group, result_record) in result_ids.iter().enumerate() {
                let group = op.results().group(result_group);
                for result_index in 0..result_record.count {
                    let use_info = UnresolvedOperand {
                        loc: result_record.loc,
                        name: ValueId::from_symbol(result_record.id)
                            .with_result_index(result_index),
                    };
                    let value = group[result_index as usize] as ValueRef;
                    self.add_definition(use_info, value);
                }
            }
        } else if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.finalize_operation_definition(op, name_span, end, &[]);
        }

        Ok(op)
    }

    /// Parse a single operation successor.
    ///
    ///   successor ::= block-id
    ///
    pub fn parse_successor(&mut self) -> ParseResult<Span<BlockRef>> {
        // Verify branch is identifier and get the matching block.
        let (span, id) = self
            .parser
            .token_stream_mut()
            .expect_map("block name", |tok| match tok {
                Token::CaretIdent(id) => Some(BlockId::from_symbol(interner::Symbol::intern(id))),
                _ => None,
            })?
            .into_parts();

        Ok(self.get_block_named(id, span))
    }

    /// Parse a comma-separated list of operation successors in brackets.
    ///
    ///   successor-list ::= `[` successor (`,` successor )* `]`
    ///
    pub fn parse_successors<const N: usize>(
        &mut self,
        destinations: &mut SmallVec<[BlockRef; N]>,
    ) -> ParseResult {
        let mut succ_ids = SmallVec::<[Span<BlockId>; N]>::new_const();

        self.parse_comma_separated_list(Delimiter::Bracket, Some("successor list"), |parser| {
            // Verify branch is identifier and get the matching block.
            let id = parser.token_stream_mut().expect_map("block name", |tok| match tok {
                Token::CaretIdent(id) => Some(BlockId::from_symbol(interner::Symbol::intern(id))),
                _ => None,
            })?;

            succ_ids.push(id);

            Ok(true)
        })?;

        if destinations.is_empty() {
            let source_id = self.source_id();
            let at = self.parser.token_stream().current_position();
            Err(ParserError::InvalidEmptySuccessorList {
                span: SourceSpan::at(source_id, at),
            })
        } else {
            Ok(())
        }
    }

    /// Parse an operation instance that is in the generic form.
    ///
    /// If `ip` is provided, operation is inserted at that point.
    pub fn parse_generic_operation(
        &mut self,
        ip: Option<ProgramPoint>,
    ) -> ParseResult<OperationRef> {
        let (span, name) = self
            .token_stream_mut()
            .expect_map("operation name", |tok| match tok {
                Token::String(name) => Some(CompactString::from(name)),
                _ => None,
            })?
            .into_parts();

        if name.is_empty() {
            return Err(ParserError::InvalidOperationName {
                span,
                reason: "operation names cannot be empty".to_string(),
            });
        }

        let Some((dialect_name, opcode)) = name.split_once('.') else {
            return Err(ParserError::InvalidOperationName {
                span,
                reason: "operation names must be fully-qualified, e.g. <dialect>.<opcode>"
                    .to_string(),
            });
        };

        // Lazy load dialects in the context as needed.
        let dialect = self
            .parser
            .context()
            .get_registered_dialect(interner::Symbol::intern(dialect_name));
        let opcode = interner::Symbol::intern(opcode);
        let Some(op_name) =
            dialect.registered_ops().iter().find(|name| name.name() == opcode).cloned()
        else {
            return Err(ParserError::InvalidOperationName {
                span,
                reason: "unable to parse unregistered operations".to_string(),
            });
        };

        // If we are populating the parser state, start a new operation definition.
        if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.start_operation_definition(&op_name);
        }

        let mut result = OperationState::new(span, op_name);
        let mut guard = CleanupOpStateRegions { state: &mut result };

        self.parse_generic_operation_after_name(&mut guard, None, None, None, None, None)?;

        // Create the operation and try to parse a location for it.
        let op = if let Some(ip) = ip {
            let mut builder = InsertionGuard::new(self.builder_mut());
            builder.set_insertion_point(ip);
            builder.create_operation(&mut guard)?
        } else {
            self.builder_mut().create_operation(&mut guard)?
        };
        self.parse_trailing_location_specifier(OpOrArgument::Op(op))?;

        self.parse_semicolon()?;

        let end = self.current_location();
        if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.finalize_operation_definition(op, span, end, &[]);
        }

        Ok(op)
    }

    /// Parse different components, viz., use-info of operand(s), successor(s), region(s),
    /// attribute(s) and function-type, of the generic form of an operation instance and populate
    /// the input operation-state 'result' with those components.
    ///
    /// If any of the components is explicitly provided, then skip parsing that component.
    pub fn parse_generic_operation_after_name(
        &mut self,
        result: &mut OperationState,
        parsed_operand_use_info: Option<&[UnresolvedOperand]>,
        parsed_successors: Option<&[BlockRef]>,
        parsed_regions: Option<&[RegionRef]>,
        parsed_attributes: Option<ParsedAttrs>,
        parsed_fn_type: Option<FunctionType>,
    ) -> ParseResult {
        // Parse the operand list, if not explicitly provided.
        let mut operand_info = SmallVec::<[UnresolvedOperand; 8]>::default();
        if let Some(provided_use_info) = parsed_operand_use_info {
            operand_info.extend_from_slice(provided_use_info);
        } else {
            self.parser.parse_lparen()?;
            self.parse_optional_ssa_use_list(&mut operand_info)?;
            self.parser.parse_rparen()?;
        }
        if !operand_info.is_empty() {
            result.operands.push(Default::default());
        }

        // Parse the successor list, if not explicitly provided.
        if let Some(provided_succs) = parsed_successors {
            result.successors.extend(provided_succs.iter().copied().enumerate().map(
                |(i, block)| PendingSuccessorInfo {
                    block,
                    key: None,
                    operand_group: (i + 1) as u8,
                },
            ));
        } else if self.parser.token_stream_mut().is_next(|tok| matches!(tok, Token::Lbracket)) {
            // Check if the operation is not a known terminator.
            if !result.name.implements::<dyn Terminator>() {
                return Err(ParserError::NonTerminatorWithSuccessors { span: result.span });
            }
            let mut successors = SmallVec::<[_; 2]>::default();
            self.parse_successors(&mut successors)?;
            result.successors.extend(successors.into_iter().enumerate().map(|(i, block)| {
                PendingSuccessorInfo {
                    block,
                    key: None,
                    operand_group: (i + 1) as u8,
                }
            }));
        }

        // Parse the region list, if not explicitly provided.
        if let Some(provided_regions) = parsed_regions {
            result.regions.extend_from_slice(provided_regions);
        } else if self.token_stream_mut().is_next(|tok| matches!(tok, Token::Lparen)) {
            // Create temporary regions with the top level region as parent.
            loop {
                let region = self.builder().context().create_region();
                self.parse_region(region, &[], /*isolated=*/ false)?;
                result.regions.push(region);
                if !self.token_stream_mut().next_if_eq(Token::Comma)? {
                    break;
                }
            }
        }

        // Parse the attributes, if not explicitly provided.
        if let Some(provided_attrs) = parsed_attributes {
            result.attrs.extend(provided_attrs);
        } else if self.token_stream_mut().is_next(|tok| matches!(tok, Token::Lbrace)) {
            self.parse_attribute_dict(&mut result.attrs)?;
        }

        // Parse the operation type, if not explicitly provided.
        let fn_ty = if let Some(provided_fn_ty) = parsed_fn_type {
            Span::new(result.span, provided_fn_ty)
        } else {
            self.parser.parse_colon()?;
            self.parser.parse_function_type()?
        };
        result.results.extend(fn_ty.results().iter().cloned());

        // Check that we have the right number of types for the operands.
        if operand_info.len() != fn_ty.arity() {
            return Err(ParserError::InvalidOperationType {
                span: result.span,
                ty_span: fn_ty.span(),
                reason: format!(
                    "expected {} operand type(s), but got {}",
                    operand_info.len(),
                    fn_ty.arity()
                ),
            });
        }

        // Resolve all of the operands.
        for (use_info, ty) in operand_info.iter().zip(fn_ty.params()) {
            let value = self.resolve_ssa_use(*use_info, ty.clone())?;
            result.operands[0].push(value);
        }

        Ok(())
    }

    /// Parse an optional trailing location and add it to the specifier Operation or
    /// [UnresolvedOperand] if present.
    ///
    ///   trailing-location ::= (`loc` (`(` location `)` | attribute-alias))?
    ///
    fn parse_trailing_location_specifier(&mut self, op_or_argument: OpOrArgument) -> ParseResult {
        // If there is a 'loc' we parse a trailing location.
        if !self.token_stream_mut().next_if_eq(Token::Loc)? {
            return Ok(());
        }

        self.parser.parse_lparen()?;

        // Check to see if we are parsing a location alias. We are parsing a location
        // alias if the token is a hash identifier *without* a dot in it - the dot
        // signifies a dialect attribute. Otherwise, we parse the location directly.
        let loc = if self
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::HashIdent(id) if !id.contains('.')))
        {
            self.parse_location_alias()?
        } else {
            self.parser.parse_location_instance()?
        };

        self.parser.parse_rparen()?;

        if let Some(span) = loc.try_into_span(self.context()) {
            op_or_argument.set_span(span);
        }

        Ok(())
    }

    /// Parse a location alias, that is a sequence looking like `#loc42`
    ///
    /// The alias may have already be defined or may be defined later, in which case an OpaqueLoc
    /// is used a placeholder. The caller must ensure that the token is actually an alias, which
    /// means it must not contain a dot.
    fn parse_location_alias(&mut self) -> ParseResult<Location> {
        let (alias_span, alias) = self
            .token_stream_mut()
            .expect_map("location alias", |tok| match tok {
                Token::HashIdent(id) => Some(id),
                _ => None,
            })?
            .into_parts();

        assert!(alias.contains('.'), "unexpected dialect attribute token, expecteed alias");

        let alias = interner::Symbol::intern(alias);
        if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.add_attr_alias_uses(alias, &[alias_span]);
        }

        // If this alias can be resolved, do it now.
        if let Some(attr) = self.state_mut().symbols.attribute_alias_definitions.get(&alias) {
            if let Ok(loc) = attr.try_downcast_attr::<LocationAttr>() {
                Ok(loc.borrow().as_value().clone())
            } else {
                Err(ParserError::InvalidLocationAlias {
                    span: alias_span,
                    reason: format!("expected location, but found '{attr:?}'"),
                })
            }
        } else {
            // Otherwise, remember this operation and resolve its location later.
            // In the meantime, use a special OpaqueLoc as a marker.
            let id = self.deferred_locs_references.len();
            self.deferred_locs_references.push(DeferredLocInfo {
                loc: alias_span,
                identifier: alias,
            });
            Ok(Location::Opaque(id))
        }
    }

    /// Parse an operation instance that is in the op-defined custom form.
    ///
    /// `results` specifies information about the "%name =" specifiers.
    pub fn parse_custom_operation(
        &mut self,
        results: &[ResultRecord],
    ) -> ParseResult<OperationRef> {
        let (name_span, name) = self.parse_custom_operation_name()?.into_parts();

        // This is the actual hook for the custom op parsing, usually implemented by the op itself
        // (`OpParser::parse()`). We retrieve it either from the OperationName or from the Dialect.
        let Some(parse_assembly_fn) = name.parse_assembly_fn() else {
            return Err(ParserError::InvalidCustomOperation {
                span: name_span,
                reason: format!("operation '{name}' does not implement OpParser"),
            });
        };
        let isolated_from_above = name.implements::<dyn IsolatedFromAbove>();
        //let default_dialect = name.default_dialect();
        // let guard = DefaultDialectStackScope::new(&mut self.default_dialect_stack);
        // guard.push(default_dialect);
        //

        let mut op_state = OperationState::new(name_span, name);

        // If we are populating the parser state, start a new operation definition.
        if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.start_operation_definition(&op_state.name);
        }

        // Have the op implementation take a crack and parsing this.
        let span = op_state.span;
        let name = op_state.name.clone();
        let mut guard = CleanupOpStateRegions {
            state: &mut op_state,
        };
        let mut custom_parser = CustomOpAsmParser::new(
            span,
            results,
            parse_assembly_fn,
            name,
            isolated_from_above,
            self,
        );
        custom_parser.parse_operation(&mut guard)?;

        // Otherwise, create the operation and try to parse a location for it.
        let op = self.builder_mut().create_operation(&mut guard)?;

        self.parse_trailing_location_specifier(OpOrArgument::Op(op))?;

        self.parse_semicolon()?;

        Ok(op)
    }

    /// Parse the name of an operation, in the custom form.
    pub fn parse_custom_operation_name(&mut self) -> ParseResult<Span<OperationName>> {
        let (name_span, name) = self
            .token_stream_mut()
            .expect_map("operation name", |tok| match tok {
                Token::BareIdent(id) => Some(id),
                _ => None,
            })?
            .into_parts();

        // If the operation doesn't have a dialect prefix try using the default dialect.
        let (dialect, opcode) = name.split_once('.').unwrap_or_else(|| {
            (self.state_mut().default_dialect_stack.last().unwrap().as_str(), name)
        });

        let dialect = self.context().get_registered_dialect(interner::Symbol::intern(dialect));
        dialect
            .registered_ops()
            .iter()
            .find(|name| name.name() == opcode)
            .cloned()
            .map(|name| Span::new(name_span, name))
            .ok_or(ParserError::UnknownOperation { span: name_span })
    }
}

// Region Parsing
impl<'input, P> OperationParser<P>
where
    P: Parser<'input>,
{
    /// Parse a region into 'region' with the provided entry block arguments.
    ///
    /// 'isolated' indicates if the naming scope of this region is isolated from those above.
    pub fn parse_region(
        &mut self,
        region: RegionRef,
        entry_arguments: &[Argument],
        isolated: bool,
    ) -> ParseResult {
        // Parse the '{'.
        self.parser.parse_lbrace()?;

        // If we are populating the parser state, start a new region definition.
        if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.start_region_definition();
        }

        // Parse the region body.
        if !entry_arguments.is_empty()
            || self.token_stream_mut().is_next(|tok| !matches!(tok, Token::Rbrace))
        {
            let start = self.parser.current_location();
            self.parse_region_body(region, start, entry_arguments, isolated)?;
        }

        self.parser.parse_rbrace()?;

        // If we are populating the parser state, finalize this region.
        if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.finalize_region_definition();
        }

        Ok(())
    }

    /// Parse a region body into 'region'.
    pub fn parse_region_body(
        &mut self,
        mut region: RegionRef,
        start: SourceSpan,
        entry_arguments: &[Argument],
        isolated: bool,
    ) -> ParseResult {
        let ip = *self.builder().insertion_point();

        // Push a new named value scope.
        self.push_ssa_name_scope(isolated);

        // Parse the first block directly to allow for it to be unnamed.
        let owning_block = self.builder().context_rc().create_block();

        // If this block is not defined in the source file, add a definition for it now in the
        // assembly state. Blocks with a name will be defined when the name is parsed.
        if !self
            .parser
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::CaretIdent(_)))
            && let Some(asm_state) = self.parser.state_mut().asm_state.as_deref_mut()
        {
            asm_state.add_block_definition(owning_block, start);
        }

        // Add arguments to the entry block if we had the form with explicit names.
        if !entry_arguments.is_empty() && entry_arguments[0].name.name.is_user_defined() {
            // If we had named arguments, then don't allow a block name.
            if self.token_stream_mut().is_next(|tok| matches!(tok, Token::CaretIdent(_))) {
                return Err(ParserError::BlockNameInRegionWithNamedArgs {
                    span: self.parser.current_location(),
                });
            }

            for arg in entry_arguments {
                let arg_info = arg.name;

                // Ensure that the argument was not already defined.
                if let Some(def_loc) = self.get_reference_loc(arg_info.name) {
                    return Err(ParserError::RegionArgumentAlreadyDefined {
                        arg: arg_info.name,
                        span: arg_info.loc,
                        prev_span: def_loc,
                    });
                }
                let arg = self
                    .context()
                    .append_block_argument(owning_block, arg.ty.clone(), arg_info.loc)
                    .borrow()
                    .downcast_ref::<BlockArgument>()
                    .unwrap()
                    .as_block_argument_ref();
                // Add a definition of this arg to the assembly state if provided.
                if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
                    asm_state.add_block_argument_definition(arg, arg_info.loc);
                }

                // Record the definition for this argument.
                self.add_definition(arg_info, arg as ValueRef)?;
            }
        }

        self.parse_block(Some(owning_block))?;

        // Verify that no other arguments were parsed.
        if !entry_arguments.is_empty()
            && owning_block.borrow().num_arguments() > entry_arguments.len()
        {
            return Err(ParserError::EntryBlockArgumentsAlreadyDefined { span: start });
        }

        // Parse the rest of the region.
        region.borrow_mut().body_mut().push_back(owning_block);

        while self.token_stream_mut().is_next(|tok| !matches!(tok, Token::Rbrace)) {
            let new_block = self.context_rc().create_block();
            self.parse_block(Some(new_block))?;
            region.borrow_mut().push_back(new_block);
        }

        // Pop the SSA value scope for this region.
        self.pop_ssa_name_scope()?;

        // Reset the original insertion point.
        self.builder_mut().restore_insertion_point(ip);

        Ok(())
    }
}

// Block Parsing
impl<'input, P> OperationParser<P>
where
    P: Parser<'input>,
{
    /// Parse a new block into 'block'.
    ///
    ///   block ::= block-label? operation*
    ///   block-label    ::= block-id block-arg-list? `:`
    ///   block-id       ::= caret-id
    ///   block-arg-list ::= `(` ssa-id-and-type-list? `)`
    ///
    pub fn parse_block(&mut self, block: Option<BlockRef>) -> ParseResult {
        // The first block of a region may already exist, if it does the caret identifier is
        // optional.
        if let Some(block) = block
            && self.token_stream_mut().is_next(|tok| !matches!(tok, Token::CaretIdent(_)))
        {
            return self.parse_block_body(block);
        }

        let (name_span, name) = self
            .token_stream_mut()
            .expect_map("block name", |tok| match tok {
                Token::CaretIdent(name) => {
                    Some(BlockId::from_symbol(interner::Symbol::intern(name)))
                }
                _ => None,
            })?
            .into_parts();

        // Define the block with the specified name.
        //
        // If a block has yet to be set, this is a new definition. If the caller provided a block,
        // use it. Otherwise create a new one.
        let block_and_loc = self.get_block_info_by_name(name);

        let block = if let Some(block) = block_and_loc {
            // Otherwise, the block has a forward declaration. Forward declarations are
            // removed once defined, so if we are defining a existing block and it is
            // not a forward declaration, then it is a redeclaration. Fail if the block
            // was already defined.
            if !self.erase_forward_ref(block.into_inner()) {
                // "redefinition of block {name}"
                return Err(ParserError::BlockAlreadyDefined {
                    span: name_span,
                    name,
                });
            }
            Span::new(name_span, block.into_inner())
        } else {
            Span::new(name_span, block.unwrap_or_else(|| self.context_rc().create_block()))
        };

        // Populate the high level assembly state if necessary.
        if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.add_block_definition(block.into_inner(), name_span);
        }

        // If an argument list is present, parse it.
        if self.token_stream_mut().is_next(|tok| matches!(tok, Token::Lparen)) {
            self.parse_optional_block_arg_list(block.into_inner())?;
        }
        self.parse_colon()?;

        // Parse the body of the block.
        self.parse_block_body(block.into_inner())
    }

    /// Parse a list of operations into 'block'.
    pub fn parse_block_body(&mut self, block: BlockRef) -> ParseResult {
        // Set the insertion point to the end of the block to parse.
        self.builder_mut().set_insertion_point_to_end(block);

        // Parse the list of operations that make up the body of the block.
        while self
            .token_stream_mut()
            .is_next(|tok| !matches!(tok, Token::CaretIdent(_) | Token::Rbrace))
        {
            self.parse_operation()?;
        }

        Ok(())
    }

    /// Parse a (possibly empty) list of SSA operands with types as block arguments
    /// enclosed in parentheses.
    ///
    ///   value-id-and-type-list ::= value-id-and-type (`,` ssa-id-and-type)*
    ///   block-arg-list ::= `(` value-id-and-type-list? `)`
    ///
    pub fn parse_optional_block_arg_list(&mut self, owner: BlockRef) -> ParseResult {
        if self.token_stream_mut().is_next(|tok| matches!(tok, Token::Rbrace)) {
            return Ok(());
        }

        // If the block already has arguments, then we're handling the entry block.
        // Parse and register the names for the arguments, but do not add them.
        let defining_existing_args = owner.borrow().has_arguments();
        let mut next_argument = 0usize;

        let context = self.context_rc();
        self.parse_comma_separated_list(Delimiter::Paren, Some("block argument list"), |parser| {
            parser.parse_ssa_def_or_use_and_type(|parser, use_info, ty| {
                // If we are defining existing arguments, ensure that the argument has already been
                // created with the right type.
                let arg = if defining_existing_args {
                    // Otherwise, ensure that this argument has already been created.
                    let owner = owner.borrow();
                    if next_argument >= owner.num_arguments() {
                        return Err(ParserError::TooManyBlockArguments { span: use_info.loc });
                    }

                    // Finally, make sure the existing argument has the correct type.
                    let arg = owner.arguments()[next_argument];
                    next_argument += 1;
                    let arg_borrowed = arg.borrow();
                    if arg_borrowed.ty() != &ty {
                        return Err(ParserError::BlockArgumentTypeMismatch {
                            span: use_info.loc,
                            arg: arg_borrowed.id(),
                            ty: arg_borrowed.ty().clone(),
                            expected: ty.clone(),
                        });
                    }
                    arg
                } else {
                    let arg = context.append_block_argument(owner, ty, use_info.loc);
                    arg.downcast::<BlockArgument, dyn Value>()
                };

                // If the argument has an explicit loc(...) specifier, parse and apply it.
                parser.parse_trailing_location_specifier(OpOrArgument::Arg(arg))?;

                // Mark this block argument definition in the parser state if it was provided.
                if let Some(asm_state) = parser.state_mut().asm_state.as_deref_mut() {
                    asm_state.add_block_argument_definition(arg, use_info.loc);
                }

                parser.add_definition(use_info, arg)
            })?;

            Ok(true)
        })
    }

    /// Get the block with the specified name, creating it if it doesn't already exist.
    ///
    /// The location specified is the point of use, which allows us to diagnose references to blocks
    /// that are not defined precisely.
    pub fn get_block_named(&mut self, name: BlockId, loc: SourceSpan) -> Span<BlockRef> {
        let block = match self.get_block_info_by_name(name) {
            Some(block) => block,
            None => {
                let block = self.context_rc().create_block();
                self.blocks_by_name.last_mut().unwrap().insert(name, Span::new(loc, block));
                self.insert_forward_ref(block, loc);
                Span::new(loc, block)
            }
        };

        if let Some(asm_state) = self.state_mut().asm_state.as_deref_mut() {
            asm_state.add_block_uses(block.into_inner(), &[loc]);
        }

        block
    }
}

impl<'input, P> OperationParser<P>
where
    P: Parser<'input>,
{
    /// Returns the info for a block at the current scope for the given name.
    fn get_block_info_by_name(&self, name: BlockId) -> Option<Span<BlockRef>> {
        self.blocks_by_name.last()?.get(&name).copied()
    }

    /// Insert a new forward reference to the given block.
    fn insert_forward_ref(&mut self, block: BlockRef, loc: SourceSpan) {
        self.forward_ref.last_mut().unwrap().insert_new(block, loc);
    }

    /// Erase any forward reference to the given block.
    fn erase_forward_ref(&mut self, block: BlockRef) -> bool {
        self.forward_ref.last_mut().unwrap().remove(&block).is_some()
    }

    /// Return true if this is a forward reference.
    fn is_forward_ref_placeholder(&self, value: ValueRef) -> bool {
        self.forward_ref_placeholders.contains_key(&value)
    }
}

impl<P> Drop for OperationParser<P> {
    fn drop(&mut self) {
        for mut op in self.forward_ref_ops.drain(..) {
            let mut op = op.borrow_mut();
            // Drop all uses of undefined forward declared reference and destroy defining operation.
            op.drop_all_uses();
            op.erase();
        }
        for scope in self.forward_ref.drain(..) {
            for (mut fwd, _) in scope.into_iter() {
                // Delete all blocks that were created as forward references but never
                // included into a region.
                fwd.borrow_mut().drop_all_uses();
            }
        }
    }
}

// RAII-style guard for cleaning up the regions in the operation state before deleting them.
//
// Within the parser, regions may get deleted if parsing failed, and other errors may be present, in
// particular undominated uses.  This makes sure such uses are deleted.
struct CleanupOpStateRegions<'a> {
    state: &'a mut OperationState,
}

impl<'a> Deref for CleanupOpStateRegions<'a> {
    type Target = OperationState;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &*self.state
    }
}

impl<'a> DerefMut for CleanupOpStateRegions<'a> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state
    }
}

impl<'a> Drop for CleanupOpStateRegions<'a> {
    fn drop(&mut self) {
        for mut region in self.state.regions.iter_mut() {
            let mut region = region.borrow_mut();
            let mut body = region.body_mut().front_mut();
            loop {
                if let Some(mut block) = body.get_mut() {
                    block.drop_all_defined_value_uses();
                } else {
                    break;
                }
                body.move_next();
            }
        }
    }
}

struct CustomOpAsmParser<'a, P> {
    span: SourceSpan,
    /// The operation name
    op_name: OperationName,
    /// The backing operation parser.
    parser: &'a mut OperationParser<P>,
    /// Information about the result name specifiers.
    result_ids: &'a [ResultRecord],
    /// The abstract information of the operation.
    parse_assembly: ParseAssemblyFn,
    isolated_from_above: bool,
}

impl<'a, 'input: 'a, P> CustomOpAsmParser<'a, P>
where
    P: Parser<'input>,
{
    pub fn new(
        span: SourceSpan,
        result_ids: &'a [ResultRecord],
        parse_assembly: ParseAssemblyFn,
        op_name: OperationName,
        isolated_from_above: bool,
        parser: &'a mut OperationParser<P>,
    ) -> Self {
        Self {
            span,
            op_name,
            parser,
            result_ids,
            parse_assembly,
            isolated_from_above,
        }
    }

    /// Parse an instance of the operation described by 'opDefinition' into the provided operation
    /// state.
    pub fn parse_operation(&mut self, state: &mut OperationState) -> ParseResult {
        let parse_assembly = self.parse_assembly;
        parse_assembly(state, self)?;

        // Verify that the parsed attributes does not have duplicate attributes.
        //
        // This can happen if an attribute set during parsing is also specified in the attribute
        // dictionary in the assembly, or the attribute is set multiple during parsing.
        state.attrs.sort_by_key(|attr| attr.name);
        for (i, attr) in state.attrs.iter().enumerate() {
            if state.attrs.get(i + 1).is_some_and(|attr2| attr.name == attr2.name) {
                return Err(ParserError::DuplicateAttribute {
                    span: self.span,
                    name: attr.name,
                });
            }
        }
        Ok(())
    }
}

impl<'a, 'input: 'a, P> Parser<'input> for CustomOpAsmParser<'a, P>
where
    P: Parser<'input>,
{
    fn builder(&self) -> &OpBuilder {
        self.parser.builder()
    }

    fn builder_mut(&mut self) -> &mut OpBuilder {
        self.parser.builder_mut()
    }

    fn state(&self) -> &ParserState<'input> {
        self.parser.state()
    }

    fn state_mut(&mut self) -> &mut ParserState<'input> {
        self.parser.state_mut()
    }

    fn token_stream(&self) -> &TokenStream<'input> {
        self.parser.token_stream()
    }

    fn token_stream_mut(&mut self) -> &mut TokenStream<'input> {
        self.parser.token_stream_mut()
    }

    fn context<'p>(&'p self) -> &'p Context
    where
        'input: 'p,
    {
        self.parser.context()
    }

    fn context_rc(&self) -> Rc<Context> {
        self.parser.context_rc()
    }

    fn source_manager<'p>(&'p self) -> &'p dyn SourceManager
    where
        'input: 'p,
    {
        self.parser.source_manager()
    }

    fn source_id(&self) -> SourceId {
        self.parser.source_id()
    }

    fn current_location(&self) -> SourceSpan {
        self.parser.current_location()
    }

    fn parse_attribute(&mut self, ty: &Type) -> ParseResult<Span<AttributeRef>> {
        self.parser.parse_attribute(ty)
    }

    fn parse_extended_attribute(&mut self, ty: &Type) -> ParseResult<Span<AttributeRef>> {
        self.parser.parse_extended_attribute(ty)
    }

    fn parse_optional_attribute(&mut self, ty: &Type) -> ParseResult<Option<Span<AttributeRef>>> {
        self.parser.parse_optional_attribute(ty)
    }

    fn parse_type(&mut self) -> ParseResult<Span<Type>> {
        self.parser.parse_type()
    }

    fn parse_optional_type(&mut self) -> ParseResult<Option<Span<Type>>> {
        self.parser.parse_optional_type()
    }

    fn parse_type_list(&mut self, result: &mut SmallVec<[Type; 4]>) -> ParseResult {
        self.parser.parse_type_list(result)
    }

    fn parse_type_list_no_parens(&mut self, result: &mut SmallVec<[Type; 4]>) -> ParseResult {
        self.parser.parse_type_list_no_parens(result)
    }

    fn parse_function_result_types(&mut self) -> ParseResult<SmallVec<[Type; 1]>> {
        self.parser.parse_function_result_types()
    }

    fn parse_dialect_symbol_body(&mut self) -> ParseResult<Span<CompactString>> {
        self.parser.parse_dialect_symbol_body()
    }

    fn parse_extended_type(&mut self) -> ParseResult<Span<Type>> {
        self.parser.parse_extended_type()
    }

    fn parse_function_type(&mut self) -> ParseResult<Span<FunctionType>> {
        self.parser.parse_function_type()
    }

    fn parse_non_function_type(&mut self) -> ParseResult<Span<Type>> {
        self.parser.parse_non_function_type()
    }

    fn parse_tuple_type(&mut self) -> ParseResult<Span<Type>> {
        self.parser.parse_tuple_type()
    }

    fn parse_attribute_dict(&mut self, attrs: &mut ParsedAttrs) -> ParseResult {
        self.parser.parse_attribute_dict(attrs)
    }

    fn parse_optional_attribute_dict(&mut self, attrs: &mut ParsedAttrs) -> ParseResult {
        self.parser.parse_optional_attribute_dict(attrs)
    }

    fn parse_optional_attribute_dict_with_keyword(
        &mut self,
        attrs: &mut ParsedAttrs,
    ) -> ParseResult {
        self.parser.parse_optional_attribute_dict_with_keyword(attrs)
    }

    fn parse_dec_or_hex_attr(
        &mut self,
        ty: &Type,
        is_negative: bool,
    ) -> ParseResult<Span<AttributeRef>> {
        self.parser.parse_dec_or_hex_attr(ty, is_negative)
    }
}

impl<'a, 'input: 'a, P> OpAsmParser<'input> for CustomOpAsmParser<'a, P>
where
    P: Parser<'input>,
{
    #[inline]
    fn parse_generic_operation(&mut self, ip: Option<ProgramPoint>) -> ParseResult<OperationRef> {
        self.parser.parse_generic_operation(ip)
    }

    #[inline]
    fn parse_generic_operation_after_name(
        &mut self,
        result: &mut OperationState,
        parsed_operand_use_info: Option<&[UnresolvedOperand]>,
        parsed_successors: Option<&[BlockRef]>,
        parsed_regions: Option<&[RegionRef]>,
        parsed_attributes: Option<ParsedAttrs>,
        parsed_fn_type: Option<FunctionType>,
    ) -> ParseResult {
        self.parser.parse_generic_operation_after_name(
            result,
            parsed_operand_use_info,
            parsed_successors,
            parsed_regions,
            parsed_attributes,
            parsed_fn_type,
        )
    }

    #[inline]
    fn parse_custom_operation_name(&mut self) -> ParseResult<Span<OperationName>> {
        self.parser.parse_custom_operation_name()
    }

    /// Return the name of the specified result in the specified syntax, as well
    /// as the subelement in the name.  For example, in this operation:
    ///
    ///  %x, %y:2, %z = foo.op
    ///
    ///    getResultName(0) == {"x", 0 }
    ///    getResultName(1) == {"y", 0 }
    ///    getResultName(2) == {"y", 1 }
    ///    getResultName(3) == {"z", 0 }
    fn get_result_name(&self, mut result_num: u8) -> Option<(interner::Symbol, u8)> {
        // Scan for the resultID that contains this result number.
        for entry in self.result_ids {
            if result_num < entry.count {
                return Some((entry.id, result_num));
            }
            result_num -= entry.count;
        }

        // Invalid result number
        None
    }

    /// Return the number of declared SSA results.  This returns 4 for the foo.op example in the
    /// comment for [Self:get_result_name].
    fn get_num_results(&self) -> usize {
        self.result_ids.iter().map(|entry| entry.count as usize).sum()
    }

    /// Parse a single operand.
    fn parse_operand(&mut self, allow_result_number: bool) -> ParseResult<UnresolvedOperand> {
        self.parser.parse_ssa_use(allow_result_number)
    }

    /// Parse a single operand if present.
    fn parse_optional_operand(
        &mut self,
        allow_result_number: bool,
    ) -> ParseResult<Option<UnresolvedOperand>> {
        if self
            .parser
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::PercentIdent(_)))
        {
            self.parse_operand(allow_result_number).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Resolve an operand to an SSA value, emitting an error on failure.
    fn resolve_operand(&mut self, operand: UnresolvedOperand, ty: Type) -> ParseResult<ValueRef> {
        self.parser.resolve_ssa_use(operand, ty)
    }

    /// Parse a single argument with the following syntax:
    ///
    ///   `%ssaname : !type { optionalAttrDict} loc(optionalSourceLoc)`
    ///
    /// If `allowType` is false or `allowAttrs` are false then the respective
    /// parts of the grammar are not parsed.
    fn parse_argument(&mut self, allow_type: bool, allow_attrs: bool) -> ParseResult<Argument> {
        let name = self.parse_operand(/*allow_result_number=*/ false)?;
        let ty = if allow_type {
            Some(self.parse_colon_type()?)
        } else {
            None
        };
        let mut attrs = SmallVec::new_const();
        if allow_attrs {
            self.parse_optional_attribute_dict(&mut attrs)?;
        }
        let loc = self.parse_optional_location_specifier()?.unwrap_or(Location::Unknown);
        Ok(Argument {
            name,
            ty: ty.map(|t| t.into_inner()).unwrap_or(Type::Unknown),
            attrs,
            loc,
        })
    }

    fn parse_optional_argument(
        &mut self,
        allow_type: bool,
        allow_attrs: bool,
    ) -> ParseResult<Option<Argument>> {
        if self
            .parser
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::PercentIdent(_)))
        {
            self.parse_argument(allow_type, allow_attrs).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Parse a region that takes `arguments` of `argTypes` types.  This
    /// effectively defines the SSA values of `arguments` and assigns their type.
    fn parse_region(
        &mut self,
        region: RegionRef,
        arguments: &[Argument],
        enable_name_shadowing: bool,
    ) -> ParseResult {
        // Try to parse the region.
        assert!(
            !enable_name_shadowing || self.isolated_from_above,
            "name shadowing is only allowed on isolated regions"
        );
        self.parser.parse_region(region, arguments, enable_name_shadowing)
    }

    /// Parses a region if present. If the region is present, a new region is
    /// allocated and placed in `region`. If no region is present, `region`
    /// remains untouched.
    fn parse_optional_region(
        &mut self,
        arguments: &[Argument],
        enable_name_shadowing: bool,
    ) -> ParseResult<Option<RegionRef>> {
        if self.parser.token_stream_mut().is_next(|tok| matches!(tok, Token::Lbrace)) {
            let new_region = self.context().create_region();
            self.parse_region(new_region, arguments, enable_name_shadowing)?;
            Ok(Some(new_region))
        } else {
            Ok(None)
        }
    }

    fn parse_successor(&mut self) -> ParseResult<Span<BlockRef>> {
        self.parser.parse_successor()
    }

    fn parse_optional_successor(&mut self) -> ParseResult<Option<Span<BlockRef>>> {
        if self
            .parser
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::CaretIdent(_)))
        {
            self.parse_successor().map(Some)
        } else {
            Ok(None)
        }
    }

    fn parse_successor_and_use_list(
        &mut self,
        operands: &mut SmallVec<[ValueRef; 2]>,
    ) -> ParseResult<Span<BlockRef>> {
        let dest = self.parse_successor()?;

        // Handle optional arguments.
        self.parse_optional_lparen()?;
        self.parser.parse_optional_ssa_use_and_type_list(operands)?;
        self.parse_rparen()?;

        Ok(dest)
    }

    /// Parse a loc(...) specifier if present, filling in result if so.
    fn parse_optional_location_specifier(&mut self) -> ParseResult<Option<Location>> {
        // If there is a 'loc' we parse a trailing location.
        if !self.parser.token_stream_mut().next_if_eq(Token::Loc)? {
            return Ok(None);
        }

        self.parser.parse_lparen()?;

        // Check to see if we are parsing a location alias. We are parsing a location alias if the
        // token is a hash identifier *without* a dot in it - the dot signifies a dialect attribute.
        // Otherwise, we parse the location directly.
        let loc = if self
            .parser
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::HashIdent(id) if !id.contains('.')))
        {
            self.parser.parse_location_alias()?
        } else {
            self.parser.parse_location_instance()?
        };

        self.parser.parse_rparen()?;

        Ok(Some(loc))
    }
}

/// This parser handles entities that are only valid at the top level of the file.
pub struct TopLevelOperationParser<P> {
    parser: P,
}

impl<'input, P> TopLevelOperationParser<P>
where
    P: Parser<'input>,
{
    pub const fn new(parser: P) -> Self {
        Self { parser }
    }

    /// Parse a set of operations into a fresh [World](crate::dialects::builtin::World)
    pub fn parse(self, loc: SourceSpan) -> ParseResult<WorldRef> {
        use crate::{BuilderExt, dialects::builtin::World};

        let Self { mut parser } = self;

        // Create a top-level operation to contain the parsed state.
        let mut top_level_op = parser.builder_mut().create::<World, ()>(loc)()?;

        let mut op_parser = OperationParser::new(parser, top_level_op);
        loop {
            let Some((start, next_token, end)) = op_parser.token_stream_mut().peek()? else {
                // If we got to the end of the file, then we're done.
                op_parser.finalize()?;
                break Ok(top_level_op);
            };

            match next_token {
                // Parse an attribute alias
                Token::HashIdent(_) => {
                    parse_attribute_alias_def(&mut op_parser)?;
                }
                // Parse a type alias
                Token::BangIdent(_) => {
                    parse_type_alias_def(&mut op_parser)?;
                }
                // Parse a file-level metadata dictionary.
                Token::FileMetadataStart => {
                    parse_file_metadata_dictionary(&mut op_parser)?;
                }
                // Parse a top-level operation
                _ => {
                    op_parser.parse_operation()?;
                }
            }
        }
    }
}

/// Parse an attribute alias declaration.
///
///   attribute-alias-def ::= '#' alias-name `=` attribute-value
///
fn parse_attribute_alias_def<'input, P>(parser: &mut OperationParser<P>) -> ParseResult
where
    P: Parser<'input>,
{
    let (span, id) = parser
        .token_stream_mut()
        .expect_map("#-identifier", |tok| match tok {
            Token::HashIdent(id) => Some(interner::Symbol::intern(id)),
            _ => None,
        })?
        .into_parts();

    // Check for redefinitions.
    if parser.state().symbols.attribute_alias_definitions.contains_key(&id) {
        return Err(ParserError::AttributeAliasAlreadyDefined { span, id });
    }

    // Make sure this isn't invading the dialect type namespace.
    if id.as_str().contains('.') {
        return Err(ParserError::InvalidAttributeAliasName {
            span,
            id,
            reason: "attribute names with a '.' are reserved for dialect-defined names".to_string(),
        });
    }

    parser.parse_equal()?;

    // Parse the attribute
    let (alias_span, attr) = parser.parse_attribute(&Type::Unknown)?.into_parts();

    // Register this alias with the parser state.
    let state = parser.state_mut();
    if let Some(asm_state) = state.asm_state.as_deref_mut() {
        asm_state.add_attr_alias_definition(id, span, Some(attr));
    }

    state.symbols.attribute_alias_definitions.insert(id, Span::new(span, attr));

    Ok(())
}

/// Parse a type alias declaration.
///
///   type-alias-def ::= '!' alias-name `=` type
///
fn parse_type_alias_def<'input, P>(parser: &mut OperationParser<P>) -> ParseResult
where
    P: Parser<'input>,
{
    let (span, id) = parser
        .token_stream_mut()
        .expect_map("!-identifier", |tok| match tok {
            Token::BangIdent(id) => Some(interner::Symbol::intern(id)),
            _ => None,
        })?
        .into_parts();

    // Check for redefinitions.
    if parser.state().symbols.type_alias_definitions.contains_key(&id) {
        return Err(ParserError::TypeAliasAlreadyDefined { span, id });
    }

    // Make sure this isn't invading the dialect type namespace.
    if id.as_str().contains('.') {
        return Err(ParserError::InvalidTypeAliasName {
            span,
            id,
            reason: "type names with a '.' are reserved for dialect-defined names".to_string(),
        });
    }

    parser.parse_equal()?;

    // Parse the type
    let (alias_span, ty) = parser.parse_type()?.into_parts();

    // Compute a span covering the whole definition
    let span = SourceSpan::new(span.source_id(), span.start()..alias_span.end());

    // Register this alias with the parser state.
    let state = parser.state_mut();
    if let Some(asm_state) = state.asm_state.as_deref_mut() {
        asm_state.add_type_alias_definition(id, span, ty.clone());
    }

    state.symbols.type_alias_definitions.insert(id, Span::new(span, ty));

    Ok(())
}

/// Parse a top-level file metadata dictionary.
///
///   file-metadata-dict ::= '{-#' file-metadata-entry* `#-}'
///
fn parse_file_metadata_dictionary<'input, P>(parser: &mut OperationParser<P>) -> ParseResult
where
    P: Parser<'input>,
{
    parser.token_stream_mut().expect(Token::FileMetadataStart)?;
    parser.parse_comma_separated_list_until(
        Token::FileMetadataEnd,
        /*allow_empty_list=*/ false,
        |parser| {
            // Parse the key of the metadata dictionary.
            let Some(key) = parser
                .parse_optional_keyword()?
                .map(|token| token.map(|t| t.into_compact_string()))
            else {
                return Ok(false);
            };
            parser.parse_colon()?;

            // Process the metadata entry
            let (span, key) = key.into_parts();
            Err(ParserError::UnknownFileMetadata { span, key })
        },
    )
}
