mod error;
mod scanner;
mod token;

use core::{num::IntErrorKind, ops::Range};

use num_traits::Num;

pub use self::{
    error::{InvalidEscapeKind, LexicalError},
    scanner::Scanner,
    token::Token,
};
use crate::{
    diagnostics::{ByteIndex, ByteOffset, SourceId, SourceSpan},
    parser::ParseError,
    Symbol, Value,
};

/// The value produced by the [Lexer] when iterated
pub type Lexed = Result<(ByteIndex, Token, ByteIndex), ParseError>;

/// Pops a single token from the [Lexer]
macro_rules! pop {
    ($lex:ident) => {{
        $lex.skip();
    }};
    ($lex:ident, $code:expr) => {{
        $lex.skip();
        $code
    }};
}

/// Pops two tokens from the [Lexer]
#[allow(unused)]
macro_rules! pop2 {
    ($lex:ident) => {{
        $lex.skip();
        $lex.skip();
    }};
    ($lex:ident, $code:expr) => {{
        $lex.skip();
        $lex.skip();
        $code
    }};
}

/// The lexer that is used to perform lexical analysis on the Miden IR grammar. The lexer implements
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
pub struct Lexer<'a> {
    source_id: SourceId,

    /// The scanner produces a sequence of chars + location, and can be controlled
    /// The location type is SourceIndex
    scanner: Scanner<'a>,

    /// The most recent token to be lexed.
    /// At the start and end, this should be Token::Eof
    token: Token,

    /// The position in the input where the current token starts
    /// At the start this will be the byte index of the beginning of the input
    token_start: ByteIndex,

    /// The position in the input where the current token ends
    /// At the start this will be the byte index of the beginning of the input
    token_end: ByteIndex,

    /// When we have reached true Eof, this gets set to true, and the only token
    /// produced after that point is Token::Eof, or None, depending on how you are
    /// consuming the lexer
    eof: bool,
}
impl<'a> Lexer<'a> {
    /// Produces an instance of the lexer with the lexical analysis to be performed on the `input`
    /// string. Note that no lexical analysis occurs until the lexer has been iterated over.
    pub fn new(source_id: SourceId, source: &'a str) -> Self {
        let scanner = Scanner::new(source);
        let mut lexer = Lexer {
            source_id,
            scanner,
            token: Token::Eof,
            token_start: 0.into(),
            token_end: 0.into(),
            eof: false,
        };
        lexer.advance();
        lexer
    }

    pub fn lex(&mut self) -> Option<<Self as Iterator>::Item> {
        if self.eof && self.token == Token::Eof {
            return None;
        }

        let token = std::mem::replace(&mut self.token, Token::Eof);
        let start = self.token_start;
        let end = self.token_end;
        self.advance();
        match token {
            Token::Error(err) => Some(Err(err.into())),
            token => Some(Ok((start, token, end))),
        }
    }

    fn advance(&mut self) {
        self.advance_start();
        self.token = self.tokenize();
    }

    #[inline]
    fn advance_start(&mut self) {
        let mut position: ByteIndex = self.scanner.position();
        loop {
            let (pos, c) = self.scanner.read().unwrap_or((position, '\0'));

            position = pos;

            if c == '\0' {
                self.eof = true;
                return;
            }

            if c.is_whitespace() {
                self.scanner.next();
                continue;
            }

            break;
        }

        self.token_start = position;
    }

    #[inline]
    fn pop(&mut self) -> char {
        let (pos, c) = self.scanner.next().unwrap_or((self.token_start, '\0'));
        self.token_end = pos + ByteOffset::from_char_len(c);
        c
    }

    #[inline]
    fn peek(&mut self) -> char {
        self.scanner.peek().map(|(_, c)| c).unwrap_or('\0')
    }

    #[inline]
    fn read(&mut self) -> char {
        self.scanner.read().map(|(_, c)| c).unwrap_or('\0')
    }

    #[inline(always)]
    fn skip(&mut self) {
        self.pop();
    }

    /// Get the span for the current token in `Source`.
    #[inline]
    fn span(&self) -> SourceSpan {
        SourceSpan::new(self.source_id, self.token_start..self.token_end)
    }

    #[inline]
    #[allow(unused)]
    fn slice_span(&self, span: impl Into<Range<usize>>) -> &str {
        self.scanner.slice(span)
    }

    /// Get a string slice of the current token.
    #[inline]
    fn slice(&self) -> &str {
        self.scanner.slice(self.span())
    }

    #[inline]
    fn skip_whitespace(&mut self) {
        let mut c: char;
        loop {
            c = self.read();

            if !c.is_whitespace() {
                break;
            }

            self.skip();
        }
    }

    fn tokenize(&mut self) -> Token {
        let c = self.read();

        if c == ';' {
            return self.lex_comment();
        }

        if c == '\0' {
            self.eof = true;
            return Token::Eof;
        }

        if c.is_whitespace() {
            self.skip_whitespace();
        }

        match self.read() {
            '.' => pop!(self, Token::Dot),
            '"' => self.lex_quoted_string(),
            '(' => pop!(self, Token::LParen),
            ')' => pop!(self, Token::RParen),
            '[' => pop!(self, Token::LBracket),
            ']' => pop!(self, Token::RBracket),
            '+' => self.lex_number(),
            '-' => self.lex_number(),
            '!' => pop!(self, Token::Bang),
            '?' => pop!(self, Token::Question),
            '#' => self.lex_symbol(),
            '0' => match self.peek() {
                'x' => {
                    self.skip();
                    self.skip();
                    self.lex_hex()
                }
                '0'..='9' => self.lex_number(),
                _ => pop!(self, Token::Int(0)),
            },
            '1'..='9' => self.lex_number(),
            'a'..='z' => self.lex_keyword_or_special_ident(),
            '_' => pop!(self, Token::Underscore),
            c => Token::Error(LexicalError::UnexpectedCharacter {
                start: self.span(),
                found: c,
            }),
        }
    }

    fn lex_comment(&mut self) -> Token {
        let mut c;
        loop {
            c = self.read();

            if c == '\n' {
                self.skip();
                break;
            }

            if c == '\0' {
                self.eof = true;
                break;
            }

            self.skip();
        }

        Token::Comment
    }

    fn lex_symbol(&mut self) -> Token {
        let c = self.pop();
        debug_assert_eq!(c, '#');

        // A '#' followed by any sequence of printable ASCII characters,
        // except whitespace, quotation marks, comma, semicolon, or params/brackets
        loop {
            match self.read() {
                c if c.is_ascii_control() => break,
                ' ' | '\'' | '"' | ',' | ';' | '[' | ']' | '(' | ')' => break,
                c if c.is_ascii_graphic() => self.skip(),
                _ => break,
            }
        }

        Token::Ident(Symbol::intern(&self.slice()[1..]))
    }

    fn lex_keyword_or_special_ident(&mut self) -> Token {
        let c = self.pop();
        debug_assert!(c.is_ascii_alphabetic() && c.is_lowercase());

        loop {
            match self.read() {
                '_' => self.skip(),
                '.' => {
                    // We only allow '.' when followed by a alpha character
                    match self.peek() {
                        c if c.is_ascii_lowercase() => self.skip(),
                        _ => break,
                    }
                }
                '0'..='9' => self.skip(),
                c if c.is_ascii_lowercase() => self.skip(),
                _ => break,
            }
        }

        let s = self.slice();
        if let Some(rest) = s.strip_prefix('v') {
            return match rest.parse::<u32>() {
                Ok(id) => Token::ValueId(Value::from_u32(id)),
                Err(_) => Token::from_keyword(s).unwrap_or_else(|| Token::Ident(Symbol::intern(s))),
            };
        }
        Token::from_keyword(s).unwrap_or_else(|| Token::Ident(Symbol::intern(s)))
    }

    fn lex_quoted_string(&mut self) -> Token {
        let quote = self.pop();
        debug_assert!(quote == '"');

        let mut buf = String::new();
        loop {
            match self.read() {
                '\0' => {
                    break Token::Error(LexicalError::UnclosedString { span: self.span() });
                }
                '"' => {
                    self.skip();
                    let symbol = Symbol::intern(&buf);

                    break Token::String(symbol);
                }
                '\\' => {
                    self.skip();
                    let start = self.token_end - 1;
                    match self.read() {
                        't' => buf.push('\t'),
                        'n' => buf.push('\n'),
                        'r' => buf.push('\r'),
                        '"' => buf.push('"'),
                        '\\' => buf.push('\\'),
                        c if c.is_ascii_hexdigit() && self.peek().is_ascii_hexdigit() => {
                            self.skip();
                            let c2 = self.read();
                            self.skip();
                            let n = c.to_digit(16).unwrap();
                            let m = c2.to_digit(16).unwrap();
                            match char::from_u32((16 * n) + m) {
                                Some(escaped) => buf.push(escaped),
                                None => {
                                    break Token::Error(LexicalError::InvalidHexEscape {
                                        span: SourceSpan::new(
                                            self.source_id,
                                            start..self.token_end,
                                        ),
                                        kind: InvalidEscapeKind::Invalid,
                                    });
                                }
                            }
                        }
                        'u' if self.peek() == '{' => {
                            let mut escape = 0u32;
                            self.skip();
                            self.skip();
                            if self.read() == '}' {
                                break Token::Error(LexicalError::InvalidUnicodeEscape {
                                    span: SourceSpan::new(self.source_id, start..self.token_end),
                                    kind: InvalidEscapeKind::Empty,
                                });
                            }
                            loop {
                                let c = self.read();
                                if !c.is_ascii_hexdigit() {
                                    match c {
                                        '}' => {
                                            self.skip();
                                            break;
                                        }
                                        '_' => {
                                            self.skip();
                                            continue;
                                        }
                                        _ => {
                                            return Token::Error(
                                                LexicalError::InvalidUnicodeEscape {
                                                    span: SourceSpan::new(
                                                        self.source_id,
                                                        (self.token_end - 1)..self.token_end,
                                                    ),
                                                    kind: InvalidEscapeKind::InvalidChars,
                                                },
                                            )
                                        }
                                    }
                                }
                                escape *= 16;
                                escape += c.to_digit(16).unwrap();
                                self.skip();
                            }
                            match char::from_u32(escape) {
                                Some(escaped) => buf.push(escaped),
                                None => {
                                    break Token::Error(LexicalError::InvalidUnicodeEscape {
                                        span: SourceSpan::new(
                                            self.source_id,
                                            start..self.token_end,
                                        ),
                                        kind: InvalidEscapeKind::Invalid,
                                    });
                                }
                            }
                        }
                        _ => {
                            break Token::Error(LexicalError::InvalidHexEscape {
                                span: SourceSpan::new(self.source_id, start..self.token_end),
                                kind: InvalidEscapeKind::InvalidChars,
                            });
                        }
                    }
                }
                c => {
                    buf.push(c);
                    self.skip();
                    continue;
                }
            }
        }
    }

    fn lex_number(&mut self) -> Token {
        use num_bigint::BigInt;

        let mut num = String::new();

        // Expect the first character to be a digit or sign
        let c = self.read();
        debug_assert!(c == '-' || c == '+' || c.is_ascii_digit());
        if c == '-' {
            num.push(self.pop());
        } else if c == '+' {
            self.skip();
        }

        while let '0'..='9' = self.read() {
            num.push(self.pop());
        }

        match num.parse::<isize>() {
            Ok(value) => Token::Int(value),
            Err(err) => match err.kind() {
                IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => {
                    let value = BigInt::from_str_radix(&num, 10).expect("invalid bigint");
                    Token::BigInt(value)
                }
                reason => Token::Error(LexicalError::InvalidInt {
                    span: self.span(),
                    reason: reason.clone(),
                }),
            },
        }
    }

    fn lex_hex(&mut self) -> Token {
        let mut res: Vec<u8> = Vec::new();

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
                return Token::Error(LexicalError::InvalidInt {
                    span: self.span(),
                    reason: IntErrorKind::InvalidDigit,
                });
            }
            self.skip();

            // Each byte is represented by two hex digits, which can be converted
            // to a value in the range 0..256 as by shifting the first left by 4
            // bits (equivalent to multiplying by 16), then adding the second digit
            let byte = (c1.to_digit(16).unwrap() << 4) + c2.to_digit(16).unwrap();
            res.push(byte as u8);
        }

        // We parse big-endian, but convert to little-endian
        res.reverse();

        Token::Hex(res.into())
    }
}

impl Iterator for Lexer<'_> {
    type Item = Lexed;

    fn next(&mut self) -> Option<Self::Item> {
        let mut res = self.lex();
        while let Some(Ok((_, Token::Comment, _))) = res {
            res = self.lex();
        }
        res
    }
}
