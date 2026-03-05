use alloc::{
    borrow::Cow,
    format,
    string::{String, ToString},
    vec,
};
use core::{num::IntErrorKind, ops::Range};

use midenc_session::diagnostics::Span;

use super::{ParseResult, ParserError, Scanner, Token};
use crate::{
    Felt,
    diagnostics::{
        ByteIndex, ByteOffset, LabeledSpan, Report, Severity, SourceId, SourceSpan,
        miette::diagnostic,
    },
};

/// The value produced by the [Lexer] when iterated
///
/// A successfully lexed token is wrapped in a tuple with the start and end byte offsets, where
/// the end offset is exclusive. We explicitly use a tuple here, and not something like Span<T>,
/// because this "triple" is the structure expected by the LALRPOP parser generator when used with
/// a custom lexer like ours.
pub type Lexed<'input> = Result<(u32, Token<'input>, u32), ParserError>;

/// Pops a single token from the [Lexer]
macro_rules! pop {
    ($lex:ident) => {{
        $lex.skip();
    }};
    ($lex:ident, $token:expr) => {{
        $lex.skip();
        Ok($token)
    }};
}

/// Pops two tokens from the [Lexer]
macro_rules! pop2 {
    ($lex:ident) => {{
        $lex.skip();
        $lex.skip();
    }};
    ($lex:ident, $token:expr) => {{
        $lex.skip();
        $lex.skip();
        Ok($token)
    }};
}

/// Pops three tokens from the [Lexer]
macro_rules! pop3 {
    ($lex:ident) => {{
        $lex.skip();
        $lex.skip();
        $lex.skip();
    }};
    ($lex:ident, $token:expr) => {{
        $lex.skip();
        $lex.skip();
        $lex.skip();
        Ok($token)
    }};
}

/// The lexer that is used to perform lexical analysis Miden Assembly grammar. The lexer implements
/// the `Iterator` trait, so in order to retrieve the tokens, you simply have to iterate over it.
///
/// # Errors
///
/// Because the lexer is implemented as an iterator over tokens, this means that you can continue
/// to get tokens even if a lexical error occurs. The lexer will attempt to recover from an error
/// by injecting tokens it expects.
///
/// If an error is unrecoverable, the lexer will continue to produce tokens, but there is no
/// guarantee that parsing them will produce meaningful results, it is primarily to assist in
/// gathering as many errors as possible.
pub struct Lexer<'input> {
    /// The [SourceId] of the file being lexed, for use in producing spans in lexer diagnostics
    source_id: SourceId,

    /// The scanner produces a sequence of chars + location, and can be controlled
    /// The location type is usize
    scanner: Scanner<'input>,

    /// The most recent token to be lexed.
    /// At the start and end, this should be Token::Eof
    token: Token<'input>,

    /// The position in the input where the current token starts
    /// At the start this will be the byte index of the beginning of the input
    token_start: usize,

    /// The position in the input where the current token ends
    /// At the start this will be the byte index of the beginning of the input
    token_end: usize,

    /// The current line number
    line_num: usize,

    /// When we have reached true Eof, this gets set to true, and the only token
    /// produced after that point is Token::Eof, or None, depending on how you are
    /// consuming the lexer
    eof: bool,
    empty: bool,

    /// If an error occurs during tokenization, it is held here
    error: Option<ParserError>,
}

impl<'input> Lexer<'input> {
    /// Produces an instance of the lexer with the lexical analysis to be performed on the `input`
    /// string. Note that no lexical analysis occurs until the lexer has been iterated over.
    pub fn new(source_id: SourceId, scanner: Scanner<'input>) -> Self {
        let start = scanner.start();
        let mut lexer = Self {
            source_id,
            scanner,
            token: Token::Eof,
            token_start: start,
            token_end: start,
            line_num: 0,
            eof: false,
            empty: false,
            error: None,
        };
        lexer.advance();
        lexer
    }

    pub fn lex(&mut self) -> Option<<Self as Iterator>::Item> {
        if let Some(err) = self.error.take() {
            return Some(Err(err));
        }

        if self.eof && matches!(self.token, Token::Eof) {
            // Emit a single Eof token at the end, then None after
            if self.empty {
                return None;
            } else {
                self.empty = true;
                let end = self.token_end as u32;
                return Some(Ok((end, Token::Eof, end)));
            }
        }

        let token = core::mem::replace(&mut self.token, Token::Eof);
        let start = self.token_start;
        let end = self.token_end;
        self.advance();
        Some(Ok((start as u32, token, end as u32)))
    }

    fn advance(&mut self) {
        self.advance_start();
        match self.tokenize() {
            Ok(tok) => {
                self.token = tok;
            }
            Err(err) => {
                self.error = Some(err);
            }
        }
    }

    fn advance_to(&mut self, pos: usize) {
        self.scanner.advance_to(pos);
        self.token_start = pos;
        match self.tokenize() {
            Ok(tok) => {
                self.token = tok;
            }
            Err(err) => {
                self.error = Some(err);
            }
        }
    }

    #[inline]
    fn advance_start(&mut self) {
        let mut position: usize;
        loop {
            let (pos, c) = self.scanner.read();

            position = pos;

            if c == '\0' {
                self.eof = true;
                return;
            }

            if c.is_whitespace() {
                if c == '\n' {
                    self.line_num += 1;
                }
                self.scanner.advance();
                continue;
            }

            break;
        }

        self.token_start = position;
    }

    #[inline]
    fn pop(&mut self) -> char {
        let (pos, c) = self.scanner.pop();
        self.token_end = pos + c.len_utf8();
        c
    }

    #[inline]
    fn peek(&mut self) -> char {
        let (_, c) = self.scanner.peek();
        c
    }

    #[inline]
    fn peek_next(&mut self) -> char {
        let (_, c) = self.scanner.peek_next();
        c
    }

    #[inline]
    fn read(&mut self) -> char {
        let (_, c) = self.scanner.read();
        c
    }

    #[inline]
    fn skip(&mut self) {
        self.pop();
    }

    /// Get the current position of the lexer in the source file
    #[inline]
    pub fn current_position(&self) -> ByteIndex {
        ByteIndex::new(self.token_end as u32)
    }

    /// Get the span for the current token in `Source`.
    #[inline]
    fn span(&self) -> SourceSpan {
        assert!(self.token_start <= self.token_end, "invalid range");
        assert!(self.token_end <= u32::MAX as usize, "file too large");
        SourceSpan::new(self.source_id, (self.token_start as u32)..(self.token_end as u32))
    }

    #[inline]
    fn slice_span(&self, span: impl Into<Range<u32>>) -> &'input str {
        let range = span.into();
        self.scanner.slice((range.start as usize)..(range.end as usize))
    }

    /// Get a string slice of the current token.
    #[inline]
    fn slice(&self) -> &'input str {
        self.slice_span(self.span())
    }

    #[inline]
    fn skip_whitespace(&mut self) {
        let mut c: char;
        loop {
            c = self.read();

            if !c.is_whitespace() {
                break;
            }

            if c == '\n' {
                self.line_num += 1;
            }

            self.skip();
        }
    }

    fn tokenize(&mut self) -> Result<Token<'input>, ParserError> {
        let c = self.read();

        if c == '/' {
            match self.peek() {
                '/' => {
                    self.skip();
                    self.skip();
                    self.skip_comment();
                    return Ok(Token::Comment);
                }
                _ => return pop!(self, Token::Slash),
            }
        }

        if c == '\0' {
            self.eof = true;
            return Ok(Token::Eof);
        }

        if c.is_whitespace() {
            self.skip_whitespace();
        }

        match self.read() {
            '@' => self.lex_at_identifier(),
            '!' | '^' | '%' => self.lex_prefixed_identifier(),
            ':' => match self.peek() {
                ':' => pop2!(self, Token::ColonColon),
                _ => pop!(self, Token::Colon),
            },
            '$' => pop!(self, Token::Dollar),
            '.' => match self.peek() {
                '.' => match self.peek_next() {
                    '.' => pop3!(self, Token::Ellipsis),
                    _ => pop!(self, Token::Dot),
                },
                _ => pop!(self, Token::Dot),
            },
            ',' => pop!(self, Token::Comma),
            ';' => pop!(self, Token::Semicolon),
            '=' => pop!(self, Token::Equal),
            '#' => match self.peek() {
                '-' => match self.peek_next() {
                    '}' => pop3!(self, Token::FileMetadataEnd),
                    _ => self.lex_prefixed_identifier(),
                },
                _ => self.lex_prefixed_identifier(),
            },
            '|' => pop!(self, Token::Pipe),
            '<' => pop!(self, Token::Langle),
            '{' => match self.peek() {
                '-' => match self.peek_next() {
                    '#' => pop3!(self, Token::FileMetadataStart),
                    _ => pop!(self, Token::Lbrace),
                },
                _ => pop!(self, Token::Lbrace),
            },
            '[' => pop!(self, Token::Lbracket),
            '(' => pop!(self, Token::Lparen),
            '>' => pop!(self, Token::Rangle),
            '}' => pop!(self, Token::Rbrace),
            ']' => pop!(self, Token::Rbracket),
            ')' => pop!(self, Token::Rparen),
            '-' => match self.peek() {
                '>' => pop2!(self, Token::Rstab),
                _ => pop!(self, Token::Minus),
            },
            '+' => pop!(self, Token::Plus),
            '*' => pop!(self, Token::Star),
            '/' => pop!(self, Token::Slash),
            '?' => pop!(self, Token::Question),
            '"' => self.lex_string(),
            '0' => match self.peek() {
                'x' => {
                    self.skip();
                    self.skip();
                    self.lex_hex()
                }
                'b' => {
                    self.skip();
                    self.skip();
                    self.lex_bin()
                }
                '0'..='9' => self.lex_decimal(),
                _ => pop!(self, Token::Int("0")),
            },
            '1'..='9' => self.lex_decimal(),
            'a'..='z' => self.lex_keyword_or_ident(),
            'A'..='Z' => self.lex_identifier(),
            '_' => self.lex_identifier(),
            invalid => Err(ParserError::InvalidCharacter {
                span: SourceSpan::at(self.source_id, self.token_start as u32),
                character: invalid,
            }),
        }
    }

    fn skip_comment(&mut self) {
        let mut c;
        loop {
            c = self.read();

            if c == '\n' {
                self.skip();
                self.line_num += 1;
                break;
            }

            if c == '\0' {
                self.eof = true;
                break;
            }

            self.skip();
        }
    }

    /// Lex an '@foo' identifier.
    ///
    ///   symbol-ref-id ::= `@` (bare-id | string-literal)
    ///
    fn lex_at_identifier(&mut self) -> Result<Token<'input>, ParserError> {
        let c = self.pop();
        debug_assert_eq!(c, '@');

        self.token_start += c.len_utf8();
        if self.read() == '"' {
            let Token::String(s) = self.lex_string()? else {
                unreachable!()
            };
            Ok(Token::AtIdent(s))
        } else {
            let Token::BareIdent(s) = self.lex_identifier()? else {
                unreachable!()
            };
            Ok(Token::AtIdent(s))
        }
    }

    fn lex_prefixed_identifier(&mut self) -> Result<Token<'input>, ParserError> {
        let prefix = self.pop();
        debug_assert!(matches!(prefix, '#' | '%' | '^' | '!'), "got '{prefix}'");

        let c = self.read();
        if c.is_ascii_digit() {
            self.token_start += prefix.len_utf8();
            while self.read().is_ascii_digit() {
                self.skip();
            }
        } else if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '$' | '.') {
            self.token_start += prefix.len_utf8();
            loop {
                match self.read() {
                    '_' | '-' | '$' | '.' => self.skip(),
                    c if c.is_ascii_alphanumeric() => self.skip(),
                    _ => break,
                }
            }
        } else {
            return Err(ParserError::UnexpectedToken {
                span: self.span(),
                token: format!("'{c}'"),
                expected: Some("expected identifier".to_string()),
            });
        }

        let id = self.slice();
        Ok(match prefix {
            '#' => Token::HashIdent(id),
            '%' => Token::PercentIdent(id),
            '^' => Token::CaretIdent(id),
            '!' => Token::BangIdent(id),
            _ => unreachable!(),
        })
    }

    fn lex_keyword_or_ident(&mut self) -> Result<Token<'input>, ParserError> {
        let c = self.pop();
        debug_assert!(c.is_ascii_alphabetic() && c.is_lowercase());

        loop {
            match self.read() {
                '_' | '-' | '$' | '.' => self.skip(),
                c if c.is_ascii_alphanumeric() => self.skip(),
                _ => break,
            }
        }

        let name = self.slice();
        Ok(Token::from_keyword_or_ident(name))
    }

    fn lex_string(&mut self) -> Result<Token<'input>, ParserError> {
        // Skip quotation mark
        self.skip();

        self.token_start += '"'.len_utf8();

        let mut is_identifier = true;
        loop {
            match self.read() {
                '\0' | '\n' => {
                    break Err(ParserError::UnclosedQuote {
                        span: SourceSpan::at(self.source_id, self.span().start()),
                    });
                }
                '\\' => {
                    is_identifier = false;
                    self.skip();
                    match self.read() {
                        '"' | '\n' => {
                            self.skip();
                        }
                        _ => (),
                    }
                }
                '"' => {
                    let span = self.span();
                    self.skip();
                    break Ok(Token::String(self.slice_span(span)));
                }
                c if c.is_alphanumeric() || c.is_ascii_graphic() => {
                    self.skip();
                }
                _ => {
                    is_identifier = false;
                    self.skip();
                }
            }
        }
    }

    fn lex_identifier(&mut self) -> Result<Token<'input>, ParserError> {
        let c = self.pop();
        debug_assert!(c.is_ascii_alphabetic() || c == '_');

        loop {
            match self.read() {
                '_' | '$' | '.' => self.skip(),
                c if c.is_ascii_alphanumeric() => self.skip(),
                _ => break,
            }
        }

        Ok(Token::BareIdent(self.slice()))
    }

    fn lex_decimal(&mut self) -> Result<Token<'input>, ParserError> {
        // Expect the first character to be a digit
        let c = self.read();
        debug_assert!(c.is_ascii_digit());

        while let '0'..='9' = self.read() {
            self.skip();
        }

        let digits = self.slice();
        Ok(Token::Int(digits))
    }

    fn lex_hex(&mut self) -> Result<Token<'input>, ParserError> {
        // Expect the first character to be a valid hexadecimal digit
        debug_assert!(self.read().is_ascii_hexdigit());

        loop {
            // If we hit a non-hex digit, we're done
            let c1 = self.read();
            if !c1.is_ascii_hexdigit() {
                break;
            }
            self.skip();

            // All hex-encoded bytes are zero-padded, and thus occur
            // in pairs, if we observe a non-hex digit at this point,
            // it is invalid
            let c2 = self.read();
            if !c2.is_ascii_hexdigit() {
                // For odd-length hex strings, we need to handle this by
                // adjusting the span to not include the unpaired character
                // and let parse_hex handle the padding
                break;
            }
            self.skip();
        }

        let span = self.span();
        let start = span.start();
        let end = span.end();
        let digit_start = start.to_u32() + 2;
        let span = SourceSpan::new(span.source_id(), start..end);
        Ok(Token::Hex(self.slice_span(digit_start..end.to_u32())))
    }

    fn lex_bin(&mut self) -> Result<Token<'input>, ParserError> {
        // Expect the first character to be a valid binary digit
        debug_assert!(is_ascii_binary(self.read()));

        loop {
            // If we hit a non-binary digit, we're done
            let c1 = self.read();
            if !is_ascii_binary(c1) {
                break;
            }
            self.skip();
        }

        let span = self.span();
        let start = span.start();
        let digit_start = start.to_u32() + 2;
        let end = span.end();
        let span = SourceSpan::new(span.source_id(), start..end);
        Ok(Token::Binary(self.slice_span(digit_start..end.to_u32())))
    }
}

/// A peekable variant of the [Lexer] iterator, which also provides access to the current position
/// in the input relative to the last token lexed.
pub struct TokenStream<'input> {
    lexer: Lexer<'input>,
    upcoming: Option<Lexed<'input>>,
    current_position: ByteIndex,
}

impl<'input> TokenStream<'input> {
    pub fn new(source_id: SourceId, scanner: Scanner<'input>) -> Self {
        let mut lexer = Lexer::new(source_id, scanner);
        let current_position = lexer.current_position();
        let upcoming = lexer.next();
        Self {
            lexer,
            upcoming,
            current_position,
        }
    }

    pub fn reset_to(&mut self, pos: usize) {
        self.lexer.advance_to(pos);
        let current_position = self.lexer.current_position();
        let upcoming = self.lexer.next();
        self.current_position = current_position;
        self.upcoming = upcoming;
    }

    /// Get the [SourceId] of the input file being tokenized
    #[inline]
    pub const fn source_id(&self) -> SourceId {
        self.lexer.source_id
    }

    /// Get the rest of the source code from the current position of the lexer
    #[inline]
    pub fn remaining_source(&self) -> &str {
        self.lexer.scanner.slice_from(self.current_position.to_usize())
    }

    /// Get the byte position of the underlying lexer.
    ///
    /// This position corresponds to the position the lexer was in after producing the last token.
    #[inline]
    pub const fn current_position(&self) -> ByteIndex {
        self.current_position
    }

    /// Get a [SourceSpan] corresponding to the current byte position of the underlying lexer.
    ///
    /// This basically wraps [`Self::current_position`] in a [SourceSpan]
    #[inline]
    pub fn current_span(&self) -> SourceSpan {
        SourceSpan::at(self.lexer.source_id, self.current_position)
    }

    /// Peek at the next token in the stream
    pub fn peek(&mut self) -> ParseResult<Option<(u32, Token<'input>, u32)>> {
        match self.upcoming.take() {
            Some(Ok(ok)) => {
                self.upcoming = Some(Ok(ok));
                Ok(Some(ok))
            }
            Some(Err(err)) => Err(err),
            None => Ok(None),
        }
    }

    /// Returns true if `predicate` returns true on the next token in the stream.
    ///
    /// If the next token is an error, this always returns false.
    pub fn is_next<F>(&mut self, predicate: F) -> bool
    where
        F: Fn(Token<'input>) -> bool,
    {
        self.upcoming.as_ref().is_some_and(|lexed| match lexed {
            Ok((_, tok, _)) => predicate(*tok),
            Err(_) => false,
        })
    }

    /// Pops the next token from the stream.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> ParseResult<Option<Span<Token<'input>>>> {
        if let Some(lexed) = self.upcoming.take() {
            self.advance();
            let source_id = self.source_id();
            lexed.map(|(start, tok, end)| Some(spanned!(source_id, start, end, tok)))
        } else {
            Ok(None)
        }
    }

    /// Pops the next token from the stream if `predicate` returns true for that token.
    pub fn next_if<F>(&mut self, predicate: F) -> ParseResult<Option<Span<Token<'input>>>>
    where
        F: Fn(Token<'input>) -> bool,
    {
        if self.is_next(predicate) {
            let source_id = self.source_id();
            let result = self
                .upcoming
                .take()
                .map(|lexed| lexed.map(|(start, tok, end)| spanned!(source_id, start, end, tok)));
            self.advance();
            result.transpose()
        } else {
            Ok(None)
        }
    }

    /// Pops the next token from the stream if `mapper` returns `Some` when applied to it.
    ///
    /// This can be used to both conditionally consume a token from the stream, while also
    /// converting the raw token that was matched in the process, avoiding redundant destructuring/
    /// matching operations.
    pub fn next_if_map<F, T>(&mut self, mapper: F) -> ParseResult<Option<Span<T>>>
    where
        F: Fn(Token<'input>) -> Option<T>,
    {
        match self.upcoming.take() {
            Some(Ok((start, tok, end))) => {
                let source_id = self.source_id();
                let result = mapper(tok).map(|out| spanned!(source_id, start, end, out));
                Ok(if result.is_some() {
                    self.advance();
                    result
                } else {
                    self.upcoming = Some(Ok((start, tok, end)));
                    None
                })
            }
            Some(Err(err)) => Err(err),
            None => Ok(None),
        }
    }

    /// Consumes the next token from the stream if it equals `expected`.
    ///
    /// Returns true if a token was consumed, or false if not.
    pub fn next_if_eq(&mut self, expected: Token<'_>) -> ParseResult<bool> {
        self.next_if(|tok| tok == expected).map(|maybe_tok| maybe_tok.is_some())
    }

    /// Consumes the next token from the stream, and expects it to equal `expected`.
    ///
    /// If the next token is not `expected`, an unexpected token error is raised.
    pub fn expect(&mut self, expected: Token<'_>) -> ParseResult {
        match self.next()? {
            Some(spanned) if spanned == expected => Ok(()),
            Some(spanned) => Err(ParserError::UnexpectedToken {
                token: spanned.to_string(),
                span: span!(spanned.span().source_id(), spanned.span().start()),
                expected: Some(expected.to_string()),
            }),
            None => Err(ParserError::UnexpectedEof {
                expected: vec![expected.to_string()],
            }),
        }
    }

    /// Consumes the next token from the stream, and expects it to match `predicate`.
    ///
    /// If `predicate` returns false for the next token, an unexpected token error is raised, using
    /// `expected` as context for that error.
    pub fn expect_if<F>(&mut self, expected: &str, predicate: F) -> ParseResult<Span<Token<'input>>>
    where
        F: Fn(Token<'input>) -> bool,
    {
        match self.next()? {
            Some(spanned) => {
                if predicate(spanned.into_inner()) {
                    Ok(spanned)
                } else {
                    Err(ParserError::UnexpectedToken {
                        span: spanned.span(),
                        token: spanned.to_string(),
                        expected: Some(expected.to_string()),
                    })
                }
            }
            None => Err(ParserError::UnexpectedEof {
                expected: vec![expected.to_string()],
            }),
        }
    }

    /// Consumes the next token from the stream, expecting it to be successfully mapped by `mapper`.
    ///
    /// If `mapper` returns `None` for the next token, an unexpected token error is raised, using
    /// `expected` as context for that error.
    pub fn expect_map<F, T>(&mut self, expected: &str, mapper: F) -> ParseResult<Span<T>>
    where
        F: Fn(Token<'input>) -> Option<T>,
    {
        match self.next()? {
            Some(spanned) => {
                let (span, tok) = spanned.into_parts();
                match mapper(tok) {
                    Some(out) => Ok(Span::new(span, out)),
                    None => Err(ParserError::UnexpectedToken {
                        span,
                        token: tok.to_string(),
                        expected: Some(expected.to_string()),
                    }),
                }
            }
            None => Err(ParserError::UnexpectedEof {
                expected: vec![expected.to_string()],
            }),
        }
    }

    #[inline]
    fn advance(&mut self) {
        self.current_position = self.lexer.current_position();
        self.upcoming = self.lexer.next();
    }
}

impl<'input> Iterator for TokenStream<'input> {
    type Item = Lexed<'input>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(lexed) = self.upcoming.take() {
            self.advance();
            Some(lexed)
        } else {
            None
        }
    }
}

impl<'input> Iterator for Lexer<'input> {
    type Item = Lexed<'input>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut res = self.lex();
        while let Some(Ok((_, Token::Comment, _))) = res {
            res = self.lex();
        }
        res
    }
}

#[inline(always)]
fn is_ascii_binary(c: char) -> bool {
    matches!(c, '0'..='1')
}
