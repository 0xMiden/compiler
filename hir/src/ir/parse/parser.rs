use super::*;
use crate::{
    ArrayType, AttributeRef, AttributeRegistration, FunctionType, ImmediateAttr, PointerType,
    StructType, SymbolNameComponent, SymbolPath, SymbolUse,
    diagnostics::{LabeledSpan, RelatedError, Severity, miette::diagnostic},
    dialects::builtin::{
        self,
        attributes::{
            BoolAttr, I8Attr, I16Attr, I32Attr, StringAttr, SymbolRefAttr, TypeAttr, U8Attr,
            U16Attr, U32Attr,
        },
    },
};

pub struct DefaultParser<'input> {
    builder: OpBuilder,
    state: ParserState<'input>,
}

impl<'input> DefaultParser<'input> {
    pub fn new(state: ParserState<'input>) -> Self {
        let builder = OpBuilder::new(state.context_rc());
        Self { builder, state }
    }
}

impl<'input> Parser<'input> for DefaultParser<'input> {
    #[inline(always)]
    fn builder(&self) -> &OpBuilder {
        &self.builder
    }

    #[inline(always)]
    fn builder_mut(&mut self) -> &mut OpBuilder {
        &mut self.builder
    }

    #[inline(always)]
    fn state(&self) -> &ParserState<'input> {
        &self.state
    }

    #[inline(always)]
    fn state_mut(&mut self) -> &mut ParserState<'input> {
        &mut self.state
    }

    #[inline(always)]
    fn token_stream(&self) -> &TokenStream<'input> {
        &self.state.token_stream
    }

    #[inline(always)]
    fn token_stream_mut(&mut self) -> &mut TokenStream<'input> {
        &mut self.state.token_stream
    }

    #[inline]
    fn parse_extended_attribute(&mut self, ty: &Type) -> ParseResult<Span<AttributeRef>> {
        parse_extended_attribute(self, ty)
    }
}

pub struct ParserImpl<P> {
    parser: P,
}

impl<'input, P> ParserImpl<P>
where
    P: Parser<'input>,
{
    pub const fn new(parser: P) -> Self {
        Self { parser }
    }
}

impl<'input, P: Parser<'input>> Parser<'input> for ParserImpl<P> {
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
        parse_extended_attribute(self, ty)
    }
}

macro_rules! token_method {
    ($name:ident, $character:literal, $tok:expr) => {
        paste::paste! {
            #[doc = concat!("Parse a '", $character, "' token.")]
            #[inline]
            fn [<parse_ $name>](&mut self) -> ParseResult {
                self.token_stream_mut().expect($tok)
            }

            #[doc = concat!("Parse a '", $character, "' token, if present.")]
            fn [<parse_optional_ $name>](&mut self) -> ParseResult<bool> {
                self.token_stream_mut().next_if_eq($tok)
            }
        }
    };
}

/// This trait provides support for parsing global entities like attributes and types.
///
/// It is intended to be derived by specialized subparsers that include state.
pub trait Parser<'input> {
    fn builder(&self) -> &OpBuilder;
    fn builder_mut(&mut self) -> &mut OpBuilder;
    fn state(&self) -> &ParserState<'input>;
    fn state_mut(&mut self) -> &mut ParserState<'input>;
    fn token_stream(&self) -> &TokenStream<'input>;
    fn token_stream_mut(&mut self) -> &mut TokenStream<'input>;

    #[inline]
    fn context<'p>(&'p self) -> &'p Context
    where
        'input: 'p,
    {
        self.state().context()
    }

    #[inline]
    fn context_rc(&self) -> Rc<Context> {
        self.state().context_rc()
    }

    #[inline]
    fn source_manager<'p>(&'p self) -> &'p dyn SourceManager
    where
        'input: 'p,
    {
        &self.context().session().source_manager
    }

    #[inline]
    fn source_id(&self) -> SourceId {
        self.token_stream().source_id()
    }

    #[inline]
    fn current_location(&self) -> SourceSpan {
        self.token_stream().current_span()
    }

    token_method!(lbrace, '{', Token::Lbrace);

    token_method!(rbrace, '}', Token::Rbrace);

    token_method!(lbracket, '[', Token::Lbracket);

    token_method!(rbracket, ']', Token::Rbracket);

    token_method!(lparen, '(', Token::Lparen);

    token_method!(rparen, ')', Token::Rparen);

    token_method!(langle, '<', Token::Langle);

    token_method!(rangle, '>', Token::Rangle);

    token_method!(arrow, "->", Token::Rstab);

    token_method!(comma, ',', Token::Comma);

    token_method!(semicolon, ';', Token::Semicolon);

    token_method!(colon, ':', Token::Colon);

    token_method!(colon_colon, "::", Token::ColonColon);

    token_method!(dot, '.', Token::Dot);

    token_method!(ellipsis, "...", Token::Ellipsis);

    token_method!(equal, '=', Token::Equal);

    token_method!(pipe, '|', Token::Pipe);

    token_method!(plus, '+', Token::Plus);

    token_method!(minus, '-', Token::Minus);

    token_method!(slash, '/', Token::Slash);

    token_method!(star, '*', Token::Star);

    token_method!(question, '?', Token::Question);

    /// Returns true if the current token corresponds to a keyword.
    fn is_current_token_a_keyword(&mut self) -> bool {
        self.token_stream_mut().is_next(|tok| match tok {
            Token::BareIdent(_) => true,
            tok => tok.is_keyword(),
        })
    }

    /// Parse a keyword
    fn parse_keyword(&mut self) -> ParseResult<Span<Token<'input>>> {
        self.token_stream_mut().expect_if("keyword", |tok| match tok {
            Token::BareIdent(_) => true,
            tok => tok.is_keyword(),
        })
    }

    /// Parse a custom keyword
    fn parse_custom_keyword(&mut self, keyword: &str) -> ParseResult<SourceSpan> {
        self.token_stream_mut()
            .expect_if(keyword, |tok| matches!(tok, Token::BareIdent(kw) if kw == keyword))
            .map(|tok| tok.span())
    }

    /// Parse a custom keyword, if present
    fn parse_optional_custom_keyword(&mut self, keyword: &str) -> ParseResult<Option<SourceSpan>> {
        self.token_stream_mut()
            .next_if(|tok| matches!(tok, Token::BareIdent(kw) if kw == keyword))
            .map(|matched| matched.map(|tok| tok.span()))
    }

    /// Parse a keyword from the given set of keyword tokens
    fn parse_keyword_from(&mut self, keywords: &[Token<'_>]) -> ParseResult<Span<CompactString>> {
        debug_assert!(
            keywords.iter().all(|kw| kw.is_keyword() || matches!(kw, Token::BareIdent(_))),
            "expected all keywords to be valid keyword tokens"
        );
        match self.token_stream_mut().next_if(|tok| keywords.contains(&tok))? {
            Some(tok) => Ok(tok.map(|tok| tok.into_compact_string())),
            None => match self.token_stream_mut().peek().unwrap() {
                Some((start, t, end)) => {
                    let expected = format!(
                        "any keyword in: {}",
                        DisplayValues::new(keywords.iter().map(|kw| kw.into_compact_string()))
                    );
                    Err(ParserError::UnexpectedToken {
                        span: SourceSpan::new(self.source_id(), start..end),
                        token: t.to_string(),
                        expected: Some(expected),
                    })
                }
                None => Err(ParserError::UnexpectedEof {
                    expected: keywords
                        .iter()
                        .map(|kw| kw.into_compact_string().into_string())
                        .collect(),
                }),
            },
        }
    }

    /// Parse a keyword, if present.
    fn parse_optional_keyword(&mut self) -> ParseResult<Option<Span<Token<'input>>>> {
        self.token_stream_mut().next_if(|tok| match tok {
            Token::BareIdent(_) => true,
            tok => tok.is_keyword(),
        })
    }

    /// Parse an optional keyword or string and return it as a [CompactString]
    fn parse_optional_keyword_or_string(&mut self) -> ParseResult<Option<Span<CompactString>>> {
        self.token_stream_mut().next_if_map(|tok| match tok {
            Token::BareIdent(s) => Some(CompactString::from(s)),
            tok if tok.is_keyword() => Some(tok.into_compact_string()),
            _ => None,
        })
    }

    /// Parse a quoted string token
    fn parse_string(&mut self) -> ParseResult<Span<CompactString>> {
        self.token_stream_mut().expect_map("string", |tok| match tok {
            Token::String(s) => Some(CompactString::from(s)),
            _ => None,
        })
    }

    /// Parse a quoted string token, if present.
    fn parse_optional_string(&mut self) -> ParseResult<Option<Span<CompactString>>> {
        self.token_stream_mut().next_if_map(|tok| match tok {
            Token::String(s) => Some(CompactString::from(s)),
            _ => None,
        })
    }

    /// Parses a Base64-encoded string of bytes.
    fn parse_base64_bytes(&mut self) -> ParseResult<Vec<u8>> {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let string =
            self.token_stream_mut().expect_map("base64-encoded string", |tok| match tok {
                Token::String(s) => Some(CompactString::from(s)),
                _ => None,
            })?;

        let mut buf = Vec::with_capacity(string.len() * 2);
        STANDARD.decode_vec(string.as_bytes(), &mut buf).map_err(|err| {
            use base64::DecodeError;

            let span = match err {
                DecodeError::InvalidByte(offset, _) | DecodeError::InvalidLastSymbol(offset, _) => {
                    span!(self.source_id(), offset as u32)
                }
                DecodeError::InvalidLength(_) | DecodeError::InvalidPadding => string.span(),
            };
            ParserError::InvalidString {
                span,
                reason: format!("invalid base64-encoding: {err}"),
            }
        })?;

        Ok(buf)
    }

    /// Parse an arbitrary attribute whose value type is `A`
    fn parse_attribute(&mut self, ty: &Type) -> ParseResult<Span<AttributeRef>> {
        if self.token_stream_mut().is_next(|tok| matches!(tok, Token::HashIdent(_))) {
            return self.parse_extended_attribute(ty);
        }

        let Some(spanned) = self.token_stream_mut().next()? else {
            return Err(ParserError::UnexpectedEof {
                expected: vec!["attribute value".to_string()],
            });
        };

        let (span, next) = spanned.into_parts();

        match next {
            Token::True | Token::False => Ok(Span::new(
                span,
                self.context_rc().create_attribute::<BoolAttr, _>(next == Token::True),
            )),
            Token::String(s) => {
                Ok(Span::new(span, self.context_rc().create_attribute::<StringAttr, _>(s)))
            }
            tok @ (Token::Int(s) | Token::Hex(s) | Token::Binary(s)) => {
                let radix = match tok {
                    Token::Int(_) => 10u32,
                    Token::Hex(_) => 16u32,
                    Token::Binary(_) => 2u32,
                    _ => unreachable!(),
                };

                let context = self.context_rc();
                match ty {
                    Type::I1 => {
                        if matches!(s, "0" | "1") {
                            Ok(Span::new(
                                span,
                                self.context_rc().create_attribute::<BoolAttr, _>(s == "1"),
                            ))
                        } else {
                            Err(ParserError::InvalidIntegerLiteral {
                                span,
                                reason: "value is out of range".to_string(),
                            })
                        }
                    }
                    Type::I8 => attr_from_str_radix::<I8Attr>(context, span, s, radix, ty),
                    Type::U8 => attr_from_str_radix::<U8Attr>(context, span, s, radix, ty),
                    Type::I16 => attr_from_str_radix::<I16Attr>(context, span, s, radix, ty),
                    Type::U16 => attr_from_str_radix::<U16Attr>(context, span, s, radix, ty),
                    Type::I32 => attr_from_str_radix::<I32Attr>(context, span, s, radix, ty),
                    Type::U32 | Type::Ptr(_) => {
                        attr_from_str_radix::<U32Attr>(context, span, s, radix, ty)
                    }
                    Type::U256 => todo!("parsing of u256 values"),
                    Type::F64 => todo!("parsing of floats"),
                    Type::Felt => Felt::try_from_str_radix(s, radix)
                        .map(|value| {
                            Span::new(
                                span,
                                self.context_rc()
                                    .create_attribute::<ImmediateAttr, _>(value)
                                    .as_attribute_ref(),
                            )
                        })
                        .map_err(|err| ParserError::InvalidIntegerLiteral {
                            span,
                            reason: err.to_string(),
                        }),
                    ty => Err(ParserError::InvalidAttributeValue {
                        span,
                        reason: format!(
                            "expected value of type {}, got integer value '{s}'",
                            TypePrinter(ty)
                        ),
                    }),
                }
            }
            Token::Question => Ok(Span::new(
                span,
                self.context_rc().create_attribute::<TypeAttr, _>(Type::Unknown),
            )),
            Token::BangIdent(ty) => {
                // Parse '!'-symbol
                todo!("parsing of custom type names")
            }
            Token::AtIdent(_) => {
                // Parse symbol reference attribute
                self.parse_symbol_ref().map(|spanned| spanned.map(|sym| sym.as_attribute_ref()))
            }
            Token::Lbracket => {
                // Parse value list
                todo!("parsing of array attributes")
            }
            Token::Lbrace => {
                // Parse value map
                todo!("parsing of dict attributes")
            }
            Token::Loc => {
                self.token_stream_mut().expect(Token::Lparen)?;
                let loc = self.parse_location_instance()?;
                self.token_stream_mut().expect(Token::Rparen)?;
                Ok(Span::new(span, self.context_rc().create_attribute::<LocationAttr, _>(loc)))
            }
            Token::AtIdent(_symbol) => {
                // Parse symbol reference attribute
                todo!("parsing of symbol references")
            }
            Token::I1 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::I1)))
            }
            Token::I8 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::I8)))
            }
            Token::U8 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::U8)))
            }
            Token::I16 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::I16)))
            }
            Token::U16 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::U16)))
            }
            Token::I32 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::I32)))
            }
            Token::U32 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::U32)))
            }
            Token::I64 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::I64)))
            }
            Token::U64 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::U64)))
            }
            Token::I128 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::I128)))
            }
            Token::U128 => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::U128)))
            }
            Token::Felt => {
                Ok(Span::new(span, self.context_rc().create_attribute::<TypeAttr, _>(Type::Felt)))
            }
            Token::Ptr => {
                self.parse_langle()?;
                let pointee = self.parse_type()?;
                self.parse_comma()?;
                let addrspace = self.token_stream_mut().expect_map(
                    "expected one of 'byte' or 'element'",
                    |tok| match tok {
                        Token::Byte => Some(AddressSpace::Byte),
                        Token::Element | Token::Felt => Some(AddressSpace::Element),
                        _ => None,
                    },
                )?;
                self.parse_rangle()?;
                Ok(Span::new(
                    span,
                    self.context_rc().create_attribute::<TypeAttr, _>(Type::Ptr(
                        PointerType::new_with_address_space(
                            pointee.into_inner(),
                            addrspace.into_inner(),
                        )
                        .into(),
                    )),
                ))
            }
            other => Err(ParserError::UnexpectedToken {
                span,
                token: other.to_string(),
                expected: Some(format!(
                    "expected one of: {}",
                    DisplayValues::new(
                        [
                            Token::String(""),
                            Token::Int(""),
                            Token::Hex(""),
                            Token::Binary(""),
                            Token::Bang,
                            Token::At,
                            Token::Lbracket,
                            Token::Lbrace,
                        ]
                        .into_iter()
                    )
                )),
            }),
        }
    }

    fn parse_extended_attribute(&mut self, ty: &Type) -> ParseResult<Span<AttributeRef>>;

    fn parse_optional_attribute(&mut self, ty: &Type) -> ParseResult<Option<Span<AttributeRef>>> {
        if self.token_stream_mut().is_next(|tok| match tok {
            Token::AtIdent(_)
            | Token::HashIdent(_)
            | Token::Int(_)
            | Token::Hex(_)
            | Token::Binary(_)
            | Token::String(_)
            | Token::Loc
            | Token::True
            | Token::False
            | Token::Lbracket
            | Token::Lbrace => true,
            Token::Byte | Token::Element => false,
            other => other.is_type_keyword(),
        }) {
            self.parse_attribute(ty).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Parse a raw location instance
    fn parse_location_instance(&mut self) -> ParseResult<Location> {
        match self
            .token_stream_mut()
            .expect_if("location instance", |tok| {
                matches!(tok, Token::String(_) | Token::BareIdent("unknown" | "synthetic"))
            })?
            .into_inner()
        {
            Token::String(s) => {
                self.parse_colon()?;
                let start_line = self.parse_decimal_integer::<u32>()?.into_inner();
                self.parse_colon()?;
                let start_col = self.parse_decimal_integer::<u32>()?.into_inner();
                let uri = Uri::new(s);
                if self
                    .token_stream_mut()
                    .next_if(|tok| matches!(tok, Token::BareIdent("to")))?
                    .is_some()
                {
                    self.parse_colon()?;
                    let end_line = self.parse_decimal_integer::<u32>()?.into_inner();
                    self.parse_colon()?;
                    let end_col = self.parse_decimal_integer::<u32>()?.into_inner();
                    Ok(Location::FileLineColRange {
                        uri,
                        start_line: LineNumber::new(start_line).unwrap_or_default(),
                        start_column: ColumnNumber::new(start_col).unwrap_or_default(),
                        end_line: LineNumber::new(end_line).unwrap_or_default(),
                        end_column: ColumnNumber::new(end_col).unwrap_or_default(),
                    })
                } else {
                    Ok(Location::FileLineCol {
                        uri,
                        line: LineNumber::new(start_line).unwrap_or_default(),
                        column: ColumnNumber::new(start_col).unwrap_or_default(),
                    })
                }
            }
            Token::BareIdent("unknown") => Ok(Location::Unknown),
            Token::BareIdent("synthetic") => Ok(Location::Synthetic),
            _ => unreachable!(),
        }
    }

    /// Parse an @-identifier and return it (without the '@') as an [Ident]
    fn parse_symbol_name(&mut self) -> ParseResult<Ident> {
        let (span, sym) = self
            .token_stream_mut()
            .expect_map("'@'-identifier", |tok| match tok {
                Token::AtIdent(s) => Some(Symbol::intern(s)),
                _ => None,
            })?
            .into_parts();
        Ok(Ident::new(sym, span))
    }

    /// Parse an @-identifier and return it (without the '@') as an [Ident]
    fn parse_optional_symbol_name(&mut self) -> ParseResult<Option<Ident>> {
        Ok(self
            .token_stream_mut()
            .next_if_map(|tok| match tok {
                Token::AtIdent(s) => Some(Symbol::intern(s)),
                _ => None,
            })?
            .map(|spanned| {
                let (span, sym) = spanned.into_parts();
                Ident::new(sym, span)
            }))
    }

    fn parse_symbol_ref(&mut self) -> ParseResult<Span<UnsafeIntrusiveEntityRef<SymbolRefAttr>>> {
        let mut components = SmallVec::<[SymbolNameComponent; 2]>::new_const();
        let start = self.current_location();
        if self.token_stream_mut().next_if_eq(Token::ColonColon)? {
            components.push(SymbolNameComponent::Root);
        }
        while let Some(component) = self.token_stream_mut().next_if_map(|tok| match tok {
            Token::AtIdent(name) => Some(interner::Symbol::intern(name)),
            _ => None,
        })? {
            if !self.token_stream_mut().next_if_eq(Token::ColonColon)? {
                components.push(SymbolNameComponent::Leaf(component.into_inner()));
                break;
            }
            components.push(SymbolNameComponent::Component(component.into_inner()));
        }
        let end = self.token_stream().current_position();
        let span = SourceSpan::new(start.source_id(), start.start()..end);

        if components.is_empty() {
            match self.token_stream_mut().peek()? {
                Some((start, tok, end)) => Err(ParserError::UnexpectedToken {
                    span: SourceSpan::new(self.source_id(), start..end),
                    token: tok.to_string(),
                    expected: Some("symbol reference".to_string()),
                }),
                None => Err(ParserError::UnexpectedEof {
                    expected: vec!["symbol reference".to_string()],
                }),
            }
        } else if matches!(components.as_slice(), [SymbolNameComponent::Root]) {
            Err(ParserError::Report(RelatedError::new(Report::from(diagnostic!(
                severity = Severity::Error,
                labels = vec![LabeledSpan::at(span, "expected at least one non-root component")],
                "invalid symbol reference"
            )))))
        } else {
            let path = SymbolPath::new(components).map_err(|err| {
                ParserError::Report(RelatedError::new(Report::from(diagnostic!(
                    severity = Severity::Error,
                    labels = vec![LabeledSpan::at(span, err.to_string())],
                    "invalid symbol reference"
                ))))
            })?;
            let attr = self.context_rc().create_attribute::<SymbolRefAttr, _>(
                builtin::attributes::SymbolRef::new(path, UnsafeIntrusiveEntityRef::dangling()),
            );
            Ok(Span::new(span, attr))
        }
    }

    /// Parse a type.
    fn parse_type(&mut self) -> ParseResult<Span<Type>> {
        let Some(ty) = self.parse_optional_type()? else {
            let (span, next) = self
                .token_stream_mut()
                .next()?
                .ok_or_else(|| ParserError::UnexpectedEof {
                    expected: vec!["type".to_string()],
                })?
                .into_parts();
            return Err(ParserError::UnexpectedToken {
                span,
                token: next.to_string(),
                expected: Some("type".to_string()),
            });
        };

        Ok(ty)
    }

    fn parse_optional_type(&mut self) -> ParseResult<Option<Span<Type>>> {
        let start = self.current_location();
        if self.token_stream_mut().next_if_eq(Token::Question)? {
            return Ok(Some(Span::new(start, Type::Unknown)));
        }

        if let Some(ty) = self.token_stream_mut().next_if_map(|tok| match tok {
            Token::I1 => Some(Type::I1),
            Token::I8 => Some(Type::I8),
            Token::U8 => Some(Type::U8),
            Token::I16 => Some(Type::I16),
            Token::U16 => Some(Type::U16),
            Token::I32 => Some(Type::I32),
            Token::U32 => Some(Type::U32),
            Token::I64 => Some(Type::I64),
            Token::U64 => Some(Type::U64),
            Token::I128 => Some(Type::I128),
            Token::U128 => Some(Type::U128),
            Token::Felt => Some(Type::Felt),
            _ => None,
        })? {
            return Ok(Some(ty));
        }

        let start = self.current_location();
        if self.token_stream_mut().next_if_eq(Token::Ptr)? {
            self.parse_langle()?;
            let pointee = self.parse_type()?;
            self.parse_comma()?;
            let addrspace =
                self.token_stream_mut().expect_map("one of 'byte' or 'felt'", |tok| match tok {
                    Token::Byte => Some(AddressSpace::Byte),
                    Token::Element | Token::Felt => Some(AddressSpace::Element),
                    _ => None,
                })?;
            self.parse_rangle()?;
            let end = self.current_location().end();
            let span = SourceSpan::new(start.source_id(), start.start()..end);
            return Ok(Some(Span::new(
                span,
                Type::Ptr(
                    PointerType::new_with_address_space(
                        pointee.into_inner(),
                        addrspace.into_inner(),
                    )
                    .into(),
                ),
            )));
        }

        if self.token_stream_mut().next_if_eq(Token::Struct)? {
            todo!()
        }

        if self.token_stream_mut().next_if_eq(Token::Array)? {
            todo!()
        }

        Ok(None)
    }

    /// Parse a type list.
    fn parse_type_list(&mut self, result: &mut SmallVec<[Type; 4]>) -> ParseResult {
        if self.token_stream_mut().is_next(|tok| matches!(tok, Token::Lparen)) {
            self.parse_comma_separated_list(Delimiter::OptionalParen, Some("type list"), |parser| {
                result.push(parser.parse_type()?.into_inner());
                Ok(true)
            })
        } else {
            let ty = self.parse_type()?;
            result.push(ty.into_inner());
            Ok(())
        }
    }

    /// Parse a type list, but without any surrounding parentheses.
    fn parse_type_list_no_parens(&mut self, result: &mut SmallVec<[Type; 4]>) -> ParseResult {
        self.parse_comma_separated_list(Delimiter::None, Some("type list"), |parser| {
            match parser.parse_optional_type()? {
                Some(ty) => {
                    result.push(ty.into_inner());
                    Ok(true)
                }
                None => Ok(false),
            }
        })
    }

    fn parse_function_result_types(&mut self) -> ParseResult<SmallVec<[Type; 1]>> {
        todo!()
    }

    /// Parse an `->` followed by a type list.
    fn parse_arrow_type_list(&mut self, result: &mut SmallVec<[Type; 4]>) -> ParseResult {
        self.parse_arrow()?;
        self.parse_type_list(result)
    }

    /// Parse an `->` followed by a type list.
    fn parse_optional_arrow_type_list(&mut self, result: &mut SmallVec<[Type; 4]>) -> ParseResult {
        if self.token_stream_mut().next_if_eq(Token::Rstab)? {
            self.parse_type_list(result)
        } else {
            Ok(())
        }
    }

    /// Parse a `:` followed by a type.
    fn parse_colon_type(&mut self) -> ParseResult<Span<Type>> {
        self.parse_colon()?;
        self.parse_type()
    }

    /// Parse a `:` followed by a type list.
    fn parse_colon_type_list(&mut self, result: &mut SmallVec<[Type; 4]>) -> ParseResult {
        self.parse_colon()?;
        self.parse_type_list(result)
    }

    /// Parse a `:` followed by a type list.
    fn parse_optional_colon_type_list(&mut self, result: &mut SmallVec<[Type; 4]>) -> ParseResult {
        if self.token_stream_mut().next_if_eq(Token::Colon)? {
            self.parse_type_list(result)
        } else {
            Ok(())
        }
    }

    /// Parse the body of a dialect symbol, which starts and ends with <>'s, and may be
    /// recursive.
    ///
    /// Return with a string encompassing the entire body.
    ///
    ///   pretty-dialect-sym-body ::= '<' pretty-dialect-sym-contents+ '>'
    ///   pretty-dialect-sym-contents ::= pretty-dialect-sym-body
    ///                                  | '(' pretty-dialect-sym-contents+ ')'
    ///                                  | '[' pretty-dialect-sym-contents+ ']'
    ///                                  | '{' pretty-dialect-sym-contents+ '}'
    ///                                  | '[^[<({>\])}\0]+'
    ///
    fn parse_dialect_symbol_body(&mut self) -> ParseResult<Span<CompactString>> {
        // Symbol bodies are a relatively unstructured format that contains a series of properly
        // nested punctuation, with anything else in the middle. Scan ahead to find it and consume
        // it if successful, otherwise emit an error.
        let start_span = self.token_stream().current_span();
        let source = self.token_stream().remaining_source();

        let mut chars = source.char_indices().peekable();
        assert_eq!(chars.peek().unwrap().1, '<');

        let mut punctuation_stack = SmallVec::<[(usize, char); 8]>::default();

        let end = loop {
            match chars.next() {
                Some((start, left @ ('(' | '[' | '{'))) => {
                    punctuation_stack.push((start, left));
                }
                Some((_, '-')) => {
                    // We treat -> as a special symbol
                    chars.next_if(|(_, c)| *c == '>');
                }
                Some((end, '>')) if punctuation_stack.is_empty() => {
                    // We've reached the end successfully
                    break end;
                }
                Some((end, '>')) => {
                    let start = punctuation_stack.last().map(|(pos, _)| *pos).unwrap_or(end);
                    if punctuation_stack.pop().is_none_or(|(_, open)| open != '<') {
                        return Err(ParserError::UnclosedDelimiter {
                            span: SourceSpan::at(
                                start_span.source_id(),
                                start_span.start().to_u32() + (start as u32),
                            ),
                            expected: '>',
                        });
                    }
                }
                Some((end, ')')) => {
                    let start = punctuation_stack.last().map(|(pos, _)| *pos).unwrap_or(end);
                    if punctuation_stack.pop().is_none_or(|(_, open)| open != '(') {
                        return Err(ParserError::UnclosedDelimiter {
                            span: SourceSpan::at(
                                start_span.source_id(),
                                start_span.start().to_u32() + (start as u32),
                            ),
                            expected: ')',
                        });
                    }
                }
                Some((end, ']')) => {
                    let start = punctuation_stack.last().map(|(pos, _)| *pos).unwrap_or(end);
                    if punctuation_stack.pop().is_none_or(|(_, open)| open != '[') {
                        return Err(ParserError::UnclosedDelimiter {
                            span: SourceSpan::at(
                                start_span.source_id(),
                                start_span.start().to_u32() + (start as u32),
                            ),
                            expected: ']',
                        });
                    }
                }
                Some((end, '}')) => {
                    let start = punctuation_stack.last().map(|(pos, _)| *pos).unwrap_or(end);
                    if punctuation_stack.pop().is_none_or(|(_, open)| open != '{') {
                        return Err(ParserError::UnclosedDelimiter {
                            span: SourceSpan::at(
                                start_span.source_id(),
                                start_span.start().to_u32() + (start as u32),
                            ),
                            expected: '}',
                        });
                    }
                }
                Some((mut last_pos, '"')) => loop {
                    match chars.next() {
                        None => {
                            return Err(ParserError::UnexpectedEof {
                                expected: vec!["'\"'".to_string()],
                            });
                        }
                        Some((_, '"')) => break,
                        Some((pos, '\\')) => {
                            last_pos = pos;
                            match chars.next() {
                                None => {
                                    return Err(ParserError::UnexpectedEof {
                                        expected: vec!["'\"'".to_string()],
                                    });
                                }
                                Some((_, '"' | '\t' | '\r' | '\n' | '\\')) => continue,
                                Some((pos, _)) => {
                                    return Err(ParserError::InvalidEscapeSequence {
                                        span: SourceSpan::at(
                                            start_span.source_id(),
                                            start_span.start().to_u32() + (pos as u32),
                                        ),
                                    });
                                }
                            }
                        }
                        Some((pos, c)) => {
                            last_pos = pos;
                        }
                    }
                },
                Some((..)) => continue,
                None => {
                    return Err(ParserError::UnexpectedEof {
                        expected: vec![format!(
                            "{}",
                            punctuation_stack.last().map(|(_, c)| *c).unwrap_or('>')
                        )],
                    });
                }
            }
        };

        // If we reached here, we've successfully parsed the body of the dialect symbol.
        // We must now manually advance the tokenizer to consume everything thus far, and
        // return the parsed string
        let body = CompactString::from_utf8(&source.as_bytes()[1..end]).unwrap();
        let span = SourceSpan::new(start_span.source_id(), start_span.start().to_u32()..end as u32);

        self.token_stream_mut().reset_to(end + 1);

        Ok(Span::new(span, body))
    }

    /// Parse an extended type.
    fn parse_extended_type(&mut self) -> ParseResult<Span<Type>> {
        todo!()
    }

    /// Parse an extended type.
    fn parse_function_type(&mut self) -> ParseResult<Span<FunctionType>> {
        let start = self.token_stream().current_position();
        let cc = if self.token_stream_mut().next_if_eq(Token::BareIdent("extern"))? {
            self.parse_lparen()?;
            let cc = self.parse_string()?;
            self.parse_rparen()?;
            let cc_span = cc.span();
            Some(match cc.as_str() {
                "C" => crate::CallConv::SystemV,
                "canon-lift" => crate::CallConv::CanonLift,
                "canon-lower" => crate::CallConv::CanonLower,
                "fast" => crate::CallConv::Fast,
                "wasm" => crate::CallConv::Wasm,
                other => {
                    return Err(ParserError::Report(RelatedError::new(Report::from(diagnostic!(
                        severity = Severity::Error,
                        labels = vec![LabeledSpan::at(
                            cc_span,
                            format!("unrecognized calling convention '{other}'")
                        )],
                        "invalid calling convention string"
                    )))));
                }
            })
        } else {
            None
        };
        let mut params = SmallVec::<[Type; _]>::default();
        self.parse_type_list(&mut params)?;
        let mut results = SmallVec::<[Type; _]>::default();
        self.parse_arrow_type_list(&mut results)?;

        let end = self.token_stream().current_span();
        let span = SourceSpan::new(end.source_id(), start..end.end());
        Ok(Span::new(span, FunctionType::new(cc.unwrap_or_default(), params, results)))
    }

    /// Parse a non function type.
    fn parse_non_function_type(&mut self) -> ParseResult<Span<Type>> {
        if let Some(tok) = self.token_stream_mut().next_if_map(|tok| tok.as_type())? {
            return Ok(tok);
        }

        if self.token_stream_mut().next_if_eq(Token::Ptr)? {
            let start = self.token_stream().current_position();
            self.parse_langle()?;
            let pointee_ty = self.parse_type()?;
            let addrspace = if self.parse_optional_comma()? {
                self.token_stream_mut()
                    .expect_map("pointer address space, e.g. byte or element", |tok| match tok {
                        Token::Byte => Some(AddressSpace::Byte),
                        Token::Element | Token::Felt => Some(AddressSpace::Element),
                        _ => None,
                    })?
                    .into_inner()
            } else {
                AddressSpace::Byte
            };
            self.parse_rangle()?;
            let end = self.token_stream().current_span();

            let span = SourceSpan::new(end.source_id(), start..end.end());
            return Ok(Span::new(
                span,
                Type::Ptr(Arc::new(PointerType::new_with_address_space(
                    pointee_ty.into_inner(),
                    addrspace,
                ))),
            ));
        }

        if self.token_stream_mut().next_if_eq(Token::Array)? {
            let start = self.token_stream().current_position();
            self.parse_langle()?;
            let element_ty = self.parse_type()?;
            self.parse_semicolon()?;
            let arity = self.parse_decimal_integer::<usize>()?;
            self.parse_rangle()?;
            let end = self.token_stream().current_span();

            let span = SourceSpan::new(end.source_id(), start..end.end());
            return Ok(Span::new(
                span,
                Type::Array(Arc::new(ArrayType::new(element_ty.into_inner(), arity.into_inner()))),
            ));
        }

        if self.token_stream_mut().next_if_eq(Token::Struct)? {
            let start = self.token_stream().current_position();
            self.parse_langle()?;

            let repr = match self.token_stream_mut().next_if_map(|tok| match tok {
                Token::BareIdent(repr @ ("transparent" | "align" | "packed")) => Some(repr),
                _ => None,
            })? {
                None => crate::TypeRepr::Default,
                Some(repr) => {
                    let repr = repr.into_inner();
                    let repr = match repr {
                        "transparent" => crate::TypeRepr::Transparent,
                        "align" => {
                            self.parse_lparen()?;
                            let alignment = self.parse_decimal_integer::<u16>()?;
                            let span = alignment.span();
                            let Some(alignment) =
                                core::num::NonZeroU16::new(alignment.into_inner())
                            else {
                                return Err(ParserError::InvalidIntegerLiteral {
                                    span,
                                    reason: "expected non-zero alignment".to_string(),
                                });
                            };
                            self.parse_rparen()?;
                            crate::TypeRepr::Align(alignment)
                        }
                        "packed" => {
                            self.parse_lparen()?;
                            let alignment = self.parse_decimal_integer::<u16>()?;
                            let span = alignment.span();
                            let Some(alignment) =
                                core::num::NonZeroU16::new(alignment.into_inner())
                            else {
                                return Err(ParserError::InvalidIntegerLiteral {
                                    span,
                                    reason: "expected non-zero alignment".to_string(),
                                });
                            };
                            self.parse_rparen()?;
                            self.parse_semicolon()?;
                            crate::TypeRepr::Packed(alignment)
                        }
                        _ => unreachable!(),
                    };
                    self.parse_semicolon()?;
                    repr
                }
            };

            let mut fields = SmallVec::<[Type; 4]>::default();
            self.parse_comma_separated_list_until(
                Token::Rangle,
                /*allow_empty=*/ false,
                |parser| {
                    let ty = parser.parse_type()?.into_inner();
                    if parser.token_stream_mut().next_if_eq(Token::BareIdent("align"))? {
                        parser.parse_lparen()?;
                        let _alignment = parser.parse_decimal_integer::<u16>()?.into_inner();
                        parser.parse_rparen()?;
                    }
                    fields.push(ty);
                    Ok(true)
                },
            )?;

            self.parse_rangle()?;
            let end = self.token_stream().current_span();

            let span = SourceSpan::new(end.source_id(), start..end.end());

            return Ok(Span::new(
                span,
                Type::Struct(Arc::new(StructType::new_with_repr(repr, fields))),
            ));
        }

        if self.token_stream_mut().is_next(|tok| matches!(tok, Token::Lparen)) {
            let ty = self.parse_tuple_type()?;
            if self.token_stream_mut().is_next(|tok| matches!(tok, Token::Rstab)) {
                return Err(ParserError::Report(
                    Report::from(diagnostic!(
                        severity = Severity::Error,
                        labels =
                            vec![LabeledSpan::at(ty.span(), "expected non-function type here")],
                        "invalid type"
                    ))
                    .into(),
                ));
            }
            return Ok(ty);
        }

        self.parse_extended_type()
    }

    /// Parse a tuple type.
    fn parse_tuple_type(&mut self) -> ParseResult<Span<Type>> {
        let mut elements = SmallVec::<[Type; 4]>::default();
        let start = self.token_stream().current_position();
        self.parse_type_list(&mut elements)?;
        let end = self.token_stream().current_span();

        let span = SourceSpan::new(end.source_id(), start..end.end());
        Ok(Span::new(span, Type::Struct(Arc::new(StructType::new(elements)))))
    }

    /// Parse an attribute dictionary.
    fn parse_attribute_dict(&mut self, attrs: &mut ParsedAttrs) -> ParseResult {
        self.parse_lbrace()?;
        self.parse_comma_separated_list_until(
            Token::Rbrace,
            /*allow_empty=*/ true,
            move |parser| {
                let key =
                    parser.token_stream_mut().expect_map("attribute dictionary key", |tok| {
                        match tok {
                            Token::BareIdent(id) => Some(interner::Symbol::intern(id)),
                            Token::String(id) => Some(interner::Symbol::intern(id)),
                            tok if tok.is_keyword() => {
                                Some(interner::Symbol::intern(tok.into_compact_string()))
                            }
                            _ => None,
                        }
                    })?;

                parser.parse_equal()?;

                let value = parser.parse_attribute(&Type::Unknown)?;

                attrs.push(NamedAttribute::new(key.into_inner(), value.into_inner()));

                Ok(true)
            },
        )
    }

    /// Parse an optional attribute dictionary.
    fn parse_optional_attribute_dict(&mut self, attrs: &mut ParsedAttrs) -> ParseResult {
        if !self.token_stream_mut().is_next(|tok| matches!(tok, Token::Lbrace)) {
            return Ok(());
        }

        self.parse_attribute_dict(attrs)
    }

    /// Parse an optional attribute dictionary, if the `attributes` keyword is present.
    fn parse_optional_attribute_dict_with_keyword(
        &mut self,
        attrs: &mut ParsedAttrs,
    ) -> ParseResult {
        if self
            .token_stream_mut()
            .next_if(|tok| matches!(tok, Token::BareIdent("attributes")))?
            .is_some()
        {
            self.parse_attribute_dict(attrs)
        } else {
            Ok(())
        }
    }

    /// Parse a decimal or a hexadecimal literal, which can be either an integer or a float
    /// attribute.
    fn parse_dec_or_hex_attr(
        &mut self,
        ty: &Type,
        is_negative: bool,
    ) -> ParseResult<Span<AttributeRef>> {
        todo!()
    }
}

pub trait ParserExt<'input>: Parser<'input> {
    /// Parse an integer value from the stream.
    fn parse_integer<I>(&mut self) -> ParseResult<Span<I>>
    where
        I: FromStrRadix,
    {
        let (span, token) = self
            .token_stream_mut()
            .expect_if("integer", |tok| {
                matches!(tok, Token::Int(_) | Token::Hex(_) | Token::Binary(_))
            })?
            .into_parts();
        let n = match token {
            Token::Int(s) => I::try_from_str_radix(s, 10),
            Token::Hex(s) => I::try_from_str_radix(s, 16),
            Token::Binary(s) => I::try_from_str_radix(s, 2),
            _ => unreachable!(),
        }
        .map_err(|err| ParserError::InvalidIntegerLiteral {
            span,
            reason: err.to_string(),
        })?;

        Ok(Span::new(span, n))
    }

    /// Parse a decimal integer value from the stream.
    fn parse_decimal_integer<I>(&mut self) -> ParseResult<Span<I>>
    where
        I: FromStrRadix,
    {
        let (span, string) = self
            .token_stream_mut()
            .expect_map("decimal integer", |tok| match tok {
                Token::Int(s) => Some(s),
                _ => None,
            })?
            .into_parts();
        let n = I::try_from_str_radix(string, 10).map_err(|err| {
            ParserError::InvalidIntegerLiteral {
                span,
                reason: err.to_string(),
            }
        })?;
        let loc = self.current_location();

        Ok(Span::new(span, n))
    }

    /// Parse an optional integer value from the stream.
    fn parse_optional_integer<I>(&mut self) -> ParseResult<Option<Span<I>>>
    where
        I: FromStrRadix,
    {
        if self
            .token_stream_mut()
            .is_next(|tok| matches!(tok, Token::Int(_) | Token::Hex(_) | Token::Binary(_)))
        {
            self.parse_integer().map(Some)
        } else {
            Ok(None)
        }
    }

    /// Parse an optional decimal integer value from the stream.
    fn parse_optional_decimal_integer<I>(&mut self) -> ParseResult<Option<Span<I>>>
    where
        I: FromStrRadix,
    {
        if self.token_stream_mut().is_next(|tok| matches!(tok, Token::Int(_))) {
            self.parse_decimal_integer().map(Some)
        } else {
            Ok(None)
        }
    }
    /// Parse a list of comma-separated items with an optional delimiter.
    ///
    /// If a delimiter is provided, then an empty list is allowed.  If not, then at least one
    /// element will be parsed.
    ///
    /// `context_message` is an optional message appended to "expected '('" sorts of diagnostics
    /// when parsing the delimeters.
    fn parse_comma_separated_list<F>(
        &mut self,
        delimiter: Delimiter,
        context_message: Option<&'static str>,
        mut element_parser: F,
    ) -> ParseResult
    where
        F: FnMut(&mut Self) -> ParseResult<bool>,
    {
        let loc = self.current_location();
        if matches!(delimiter, Delimiter::None) {
            while element_parser(self)? {
                if !self.parse_optional_comma()? {
                    break;
                }
            }
            Ok(())
        } else if self.token_stream_mut().next_if(|c| delimiter.is_open(&c))?.is_some() {
            // Empty list
            if self.token_stream_mut().next_if(|c| delimiter.is_close(&c))?.is_some() {
                return Ok(());
            }

            while element_parser(self)? {
                if !self.parse_optional_comma()? {
                    break;
                }
            }
            self.token_stream_mut()
                .expect_if(delimiter.expected_close(), |tok| delimiter.is_close(&tok))?;
            Ok(())
        } else if delimiter.is_optional() {
            Ok(())
        } else {
            Err(self.token_stream_mut().expect(delimiter.open().unwrap()).unwrap_err())
        }
    }

    /// Parse a comma-separated list of elements up until the specified end token.
    fn parse_comma_separated_list_until<F>(
        &mut self,
        end_token: Token<'_>,
        allow_empty_list: bool,
        mut element_parser: F,
    ) -> ParseResult
    where
        F: FnMut(&mut Self) -> ParseResult<bool>,
    {
        if self.token_stream_mut().next_if_eq(end_token)? {
            let loc = self.current_location();
            if allow_empty_list {
                Ok(())
            } else {
                Err(ParserError::UnexpectedToken {
                    span: loc,
                    token: end_token.to_string(),
                    expected: Some("at least one element".to_string()),
                })
            }
        } else {
            while element_parser(self)? {
                if !self.parse_optional_comma()? {
                    break;
                }
            }
            self.token_stream_mut().expect(end_token)
        }
    }

    fn parse_custom_type_with_fallback<F>(&mut self, parse_type: F) -> ParseResult<Span<Type>>
    where
        F: Fn(&mut Self) -> ParseResult<Span<Type>>,
    {
        if self.token_stream_mut().is_next(|tok| matches!(tok, Token::BangIdent(_))) {
            self.parse_type()
        } else {
            parse_type(self)
        }
    }

    fn parse_custom_attribute_with_fallback<F>(
        &mut self,
        ty: &Type,
        parse_attribute: F,
    ) -> ParseResult<Span<AttributeRef>>
    where
        F: Fn(&mut Self) -> ParseResult<Span<AttributeRef>>,
    {
        if self.token_stream_mut().is_next(|tok| matches!(tok, Token::HashIdent(_))) {
            self.parse_attribute(ty)
        } else {
            parse_attribute(self)
        }
    }

    /// Parse an optional attribute that is demarcated by a specific token.
    fn parse_optional_attribute_with_token<T>(
        &mut self,
        token: Token<'_>,
        ty: &Type,
    ) -> ParseResult<Option<Span<UnsafeIntrusiveEntityRef<T>>>>
    where
        T: AttributeRegistration,
    {
        use crate::any::AsAny;

        if self.token_stream_mut().is_next(|tok| tok == token) {
            return Ok(None);
        }

        let (span, value) = self.parse_attribute(ty)?.into_parts();

        match value.try_downcast::<T>() {
            Ok(attr) => Ok(Some(Span::new(span, attr))),
            Err(other) => {
                let other = other.borrow();
                let other_name = (&*other as &dyn AsAny).type_name();
                Err(ParserError::InvalidAttributeValue {
                    span,
                    reason: format!(
                        "expected attribute of type {}, got {other_name}",
                        core::any::type_name::<T>(),
                    ),
                })
            }
        }
    }

    fn parse_typed_attribute<T>(&mut self, ty: &Type) -> ParseResult<Span<AttributeRef>>
    where
        T: AttributeRegistration,
    {
        let name = self.context().get_registered_attribute_name::<T>();
        let default_dialect = *self.state().default_dialect_stack.last().unwrap();
        let allow_dialect_elision = name.dialect() == default_dialect;
        let tok =
            self.token_stream_mut()
                .expect_if(&format!("{name} attribute"), |tok| match tok {
                    Token::HashIdent(id) => {
                        id == name.dialect().as_str()
                            || allow_dialect_elision && id == name.name().as_str()
                    }
                    _ => false,
                })?;

        let attr = self.parse_extended_attribute(ty)?;

        if attr.borrow().is::<T>() {
            Ok(attr)
        } else {
            Err(ParserError::InvalidAttributeValue {
                span: attr.span(),
                reason: format!("expected attribute of type '{name}'"),
            })
        }
    }
}

impl<'input, P: ?Sized + Parser<'input>> ParserExt<'input> for P {}

fn attr_from_str_radix<A>(
    context: Rc<Context>,
    span: SourceSpan,
    s: &str,
    radix: u32,
    ty: &Type,
) -> ParseResult<Span<AttributeRef>>
where
    A: AttributeRegistration,
    <A as AttributeRegistration>::Value: FromStrRadix,
{
    let parsed =
        <<A as AttributeRegistration>::Value as FromStrRadix>::try_from_str_radix(s, radix)
            .map_err(|err| ParserError::InvalidIntegerLiteral {
                span,
                reason: err.to_string(),
            })?;

    let attr = context.create_attribute_with_type::<A, _>(parsed, ty.clone());
    Ok(Span::new(span, attr))
}

pub(super) fn parse_extended_attribute<'input, P: Parser<'input>>(
    parser: &mut P,
    ty: &Type,
) -> ParseResult<Span<AttributeRef>> {
    let (name_span, name) = parser
        .token_stream_mut()
        .expect_map("attribute name", |tok| match tok {
            Token::HashIdent(id) => Some(interner::Symbol::intern(id)),
            _ => None,
        })?
        .into_parts();

    let (dialect, name) = name
        .as_str()
        .split_once('.')
        .unwrap_or((parser.state().default_dialect_stack.last().unwrap().as_str(), name.as_str()));

    let dialect = parser.context().get_registered_dialect(dialect);
    let Some(name) = dialect.registered_attrs().iter().find(|attr| attr.name() == name).cloned()
    else {
        return Err(ParserError::UnknownAttribute { span: name_span });
    };

    if parser.token_stream_mut().next_if_eq(Token::Langle)? {
        if let Some(attr_parser) = name.parse_assembly_fn() {
            let attr = attr_parser(parser)?;
            parser.parse_rangle()?;
            let end = parser.token_stream().current_position();
            let span = SourceSpan::new(name_span.source_id(), name_span.start()..end);
            Ok(Span::new(span, attr))
        } else {
            todo!("AttrParser is not yet implemented for '{name}'")
        }
    } else {
        let mut value = name.create_default(&parser.context_rc());
        if !matches!(ty, Type::Unknown) {
            value.borrow_mut().set_type(ty.clone());
        }
        Ok(Span::new(name_span, value))
    }
}
