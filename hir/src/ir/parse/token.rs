use alloc::string::String;
use core::fmt;

use miden_core::{
    Felt, FieldElement, StarkField,
    utils::{ByteReader, ByteWriter, Deserializable, DeserializationError, Serializable},
};

use crate::{CompactString, Type};

/// The token type produced by [crate::parser::Lexer], and consumed by the parser.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Token<'input> {
    // Markers
    Comment,
    Eof,

    // Identifiers
    /// foo
    BareIdent(&'input str),
    /// @foo
    AtIdent(&'input str),
    /// #foo
    HashIdent(&'input str),
    /// %foo
    PercentIdent(&'input str),
    /// ^foo
    CaretIdent(&'input str),
    /// !foo
    BangIdent(&'input str),

    // Primitives
    /// "foo"
    String(&'input str),
    /// 128
    Int(&'input str),
    /// 0x0F
    Hex(&'input str),
    /// 0b00001111
    Binary(&'input str),

    // Value Keywords
    True,
    False,

    // Attribute Keywords
    Loc,

    // Type Keywords
    I1,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    I128,
    U128,
    Felt,
    Never,
    Ptr,
    Struct,
    List,
    Array,
    Byte,
    Element,

    // Delimiters
    /// <
    Langle,
    /// >
    Rangle,
    /// {
    Lbrace,
    /// }
    Rbrace,
    /// [
    Lbracket,
    /// ]
    Rbracket,
    /// (
    Lparen,
    /// )
    Rparen,
    /// {-#
    FileMetadataStart,
    /// #-}
    FileMetadataEnd,

    // Punctuation
    /// @
    At,
    /// !
    Bang,
    /// ,
    Comma,
    /// ::
    ColonColon,
    /// :
    Colon,
    /// .
    Dot,
    /// $
    Dollar,
    /// ...
    Ellipsis,
    /// =
    Equal,
    /// #
    Hash,
    /// %
    Percent,
    /// |
    Pipe,
    /// +
    Plus,
    /// ;
    Semicolon,
    /// /
    Slash,
    /// *
    Star,
    /// -
    Minus,
    /// ->
    Rstab,
    /// ?
    Question,
}

impl Token<'_> {
    pub fn into_compact_string(&self) -> CompactString {
        match self {
            Self::BareIdent(s)
            | Self::AtIdent(s)
            | Self::HashIdent(s)
            | Self::PercentIdent(s)
            | Self::CaretIdent(s)
            | Self::BangIdent(s)
            | Self::String(s)
            | Self::Int(s)
            | Self::Hex(s)
            | Self::Binary(s) => CompactString::from(*s),
            Self::Comment => CompactString::const_new("comment"),
            Self::Eof => CompactString::const_new("end of input"),
            Self::True => CompactString::const_new("true"),
            Self::False => CompactString::const_new("false"),
            Self::Loc => CompactString::const_new("loc"),
            Self::I1 => CompactString::const_new("i1"),
            Self::I8 => CompactString::const_new("i8"),
            Self::U8 => CompactString::const_new("u8"),
            Self::I16 => CompactString::const_new("i16"),
            Self::U16 => CompactString::const_new("u16"),
            Self::I32 => CompactString::const_new("i32"),
            Self::U32 => CompactString::const_new("u32"),
            Self::I64 => CompactString::const_new("i61"),
            Self::U64 => CompactString::const_new("u64"),
            Self::I128 => CompactString::const_new("i128"),
            Self::U128 => CompactString::const_new("u128"),
            Self::Felt => CompactString::const_new("felt"),
            Self::Never => CompactString::const_new("never"),
            Self::Ptr => CompactString::const_new("ptr"),
            Self::Struct => CompactString::const_new("struct"),
            Self::Array => CompactString::const_new("array"),
            Self::List => CompactString::const_new("list"),
            Self::Byte => CompactString::const_new("byte"),
            Self::Element => CompactString::const_new("element"),
            Self::Langle => CompactString::const_new("<"),
            Self::Rangle => CompactString::const_new(">"),
            Self::Lbrace => CompactString::const_new("{"),
            Self::Rbrace => CompactString::const_new("}"),
            Self::Lbracket => CompactString::const_new("["),
            Self::Rbracket => CompactString::const_new("]"),
            Self::Lparen => CompactString::const_new("("),
            Self::Rparen => CompactString::const_new(")"),
            Self::FileMetadataStart => CompactString::const_new("{-#"),
            Self::FileMetadataEnd => CompactString::const_new("#-}"),
            Self::At => CompactString::const_new("@"),
            Self::Bang => CompactString::const_new("!"),
            Self::Comma => CompactString::const_new(","),
            Self::ColonColon => CompactString::const_new("::"),
            Self::Colon => CompactString::const_new(":"),
            Self::Dot => CompactString::const_new("."),
            Self::Dollar => CompactString::const_new("$"),
            Self::Ellipsis => CompactString::const_new("..."),
            Self::Equal => CompactString::const_new("="),
            Self::Hash => CompactString::const_new("#"),
            Self::Percent => CompactString::const_new("%"),
            Self::Pipe => CompactString::const_new("|"),
            Self::Plus => CompactString::const_new("+"),
            Self::Semicolon => CompactString::const_new(";"),
            Self::Slash => CompactString::const_new("/"),
            Self::Star => CompactString::const_new("*"),
            Self::Minus => CompactString::const_new("-"),
            Self::Rstab => CompactString::const_new("->"),
            Self::Question => CompactString::const_new("?"),
        }
    }
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use core::fmt::Write;
        match self {
            Self::BareIdent(_) => f.write_str("bare identifier"),
            Self::AtIdent(_) => f.write_str("'@'-identifier"),
            Self::HashIdent(_) => f.write_str("'#'-identifier"),
            Self::PercentIdent(_) => f.write_str("'%'-identifier"),
            Self::CaretIdent(_) => f.write_str("'^'-identifier"),
            Self::BangIdent(_) => f.write_str("'!'-identifier"),
            Self::String(_) => f.write_str("string"),
            Self::Int(_) => f.write_str("integer"),
            Self::Hex(_) => f.write_str("hex-encoded integer"),
            Self::Binary(_) => f.write_str("binary-encoded integer"),
            Self::Comment => f.write_str("comment"),
            Self::Eof => f.write_str("end of input"),
            Self::True => f.write_str("true"),
            Self::False => f.write_str("false"),
            Self::Loc => f.write_str("loc"),
            Self::I1 => f.write_str("i1"),
            Self::I8 => f.write_str("i8"),
            Self::U8 => f.write_str("u8"),
            Self::I16 => f.write_str("i16"),
            Self::U16 => f.write_str("u16"),
            Self::I32 => f.write_str("i32"),
            Self::U32 => f.write_str("u32"),
            Self::I64 => f.write_str("i61"),
            Self::U64 => f.write_str("u64"),
            Self::I128 => f.write_str("i128"),
            Self::U128 => f.write_str("u128"),
            Self::Felt => f.write_str("felt"),
            Self::Never => f.write_str("never"),
            Self::Ptr => f.write_str("ptr"),
            Self::Struct => f.write_str("struct"),
            Self::Array => f.write_str("array"),
            Self::List => f.write_str("list"),
            Self::Byte => f.write_str("byte"),
            Self::Element => f.write_str("element"),
            Self::Langle => f.write_char('<'),
            Self::Rangle => f.write_char('>'),
            Self::Lbrace => f.write_char('{'),
            Self::Rbrace => f.write_char('}'),
            Self::Lbracket => f.write_char('['),
            Self::Rbracket => f.write_char(']'),
            Self::Lparen => f.write_char('('),
            Self::Rparen => f.write_char(')'),
            Self::FileMetadataStart => f.write_str("{-#"),
            Self::FileMetadataEnd => f.write_str("#-}"),
            Self::At => f.write_char('@'),
            Self::Bang => f.write_char('!'),
            Self::Comma => f.write_char(','),
            Self::ColonColon => f.write_str("::"),
            Self::Colon => f.write_char(':'),
            Self::Dot => f.write_char('.'),
            Self::Dollar => f.write_char('$'),
            Self::Ellipsis => f.write_str("..."),
            Self::Equal => f.write_char('='),
            Self::Hash => f.write_char('#'),
            Self::Percent => f.write_char('%'),
            Self::Pipe => f.write_char('|'),
            Self::Plus => f.write_char('+'),
            Self::Semicolon => f.write_char(';'),
            Self::Slash => f.write_char('/'),
            Self::Star => f.write_char('*'),
            Self::Minus => f.write_char('-'),
            Self::Rstab => f.write_str("->"),
            Self::Question => f.write_char('?'),
        }
    }
}

impl<'input> Token<'input> {
    /// Returns true if this token represents the name of an type or a type-related keyword.
    ///
    /// This is used to simplify diagnostic output related to expected tokens so as not to
    /// overwhelm the user with a ton of possible expected tokens.
    pub fn is_type_keyword(&self) -> bool {
        matches!(
            self,
            Token::I1
                | Token::I8
                | Token::I16
                | Token::I32
                | Token::I64
                | Token::I128
                | Token::U8
                | Token::U16
                | Token::U32
                | Token::U64
                | Token::U128
                | Token::Felt
                | Token::Ptr
                | Token::Array
                | Token::List
                | Token::Struct
        )
    }

    pub fn as_type(&self) -> Option<Type> {
        match self {
            Token::I1 => Some(Type::I1),
            Token::I8 => Some(Type::I8),
            Token::I16 => Some(Type::I16),
            Token::I32 => Some(Type::I32),
            Token::I64 => Some(Type::I64),
            Token::I128 => Some(Type::I128),
            Token::U8 => Some(Type::U8),
            Token::U16 => Some(Type::U16),
            Token::U32 => Some(Type::U32),
            Token::U64 => Some(Type::U64),
            Token::U128 => Some(Type::U128),
            Token::Felt => Some(Type::Felt),
            _ => None,
        }
    }

    pub fn is_keyword(&self) -> bool {
        matches!(self, Token::True | Token::False | Token::Loc | Token::Byte | Token::Element)
            || self.is_type_keyword()
    }

    /// Returns true if this token represents a known delimiter, e.g. `(`
    pub fn is_delimiter(&self) -> bool {
        matches!(
            self,
            Self::Lparen
                | Self::Rparen
                | Self::Lbracket
                | Self::Rbracket
                | Self::Langle
                | Self::Rangle
                | Self::Lbrace
                | Self::Rbrace
        )
    }

    /// Returns an appropriate [Token] depending on whether the given string is a keyword or an
    /// identifier.
    pub fn from_keyword_or_ident(s: &'input str) -> Self {
        match s {
            "true" => Self::True,
            "false" => Self::False,
            "loc" => Self::Loc,
            "i1" => Self::I1,
            "i8" => Self::I8,
            "u8" => Self::U8,
            "i16" => Self::I16,
            "u16" => Self::U16,
            "i32" => Self::I32,
            "u32" => Self::U32,
            "i64" => Self::I64,
            "u64" => Self::U64,
            "i128" => Self::I128,
            "u128" => Self::U128,
            "felt" => Self::Felt,
            "ptr" => Self::Ptr,
            "struct" => Self::Struct,
            "array" => Self::Array,
            "list" => Self::List,
            "byte" => Self::Byte,
            "element" => Self::Element,
            _ => Self::BareIdent(s),
        }
    }

    /// Parses a [Token] from a string corresponding to that token.
    ///
    /// This solely exists to aid in constructing more user-friendly error messages in certain
    /// scenarios, and is otherwise not used (nor should it be).
    pub fn parse(s: &'input str) -> Option<Token<'input>> {
        match Token::from_keyword_or_ident(s) {
            Token::BareIdent(_) => {
                // Nope, try again
                match s {
                    "<" => Some(Token::Langle),
                    "(" => Some(Token::Lparen),
                    "{" => Some(Token::Lbrace),
                    "[" => Some(Token::Lbracket),
                    ">" => Some(Token::Rangle),
                    ")" => Some(Token::Rparen),
                    "}" => Some(Token::Rbrace),
                    "]" => Some(Token::Rbracket),
                    "@" => Some(Token::At),
                    "!" => Some(Token::Bang),
                    ":" => Some(Token::Colon),
                    "::" => Some(Token::ColonColon),
                    "." => Some(Token::Dot),
                    "$" => Some(Token::Dollar),
                    "," => Some(Token::Comma),
                    ";" => Some(Token::Semicolon),
                    "..." => Some(Token::Ellipsis),
                    "=" => Some(Token::Equal),
                    "#" => Some(Token::Hash),
                    "%" => Some(Token::Percent),
                    "|" => Some(Token::Pipe),
                    "+" => Some(Token::Plus),
                    "-" => Some(Token::Minus),
                    "/" => Some(Token::Slash),
                    "*" => Some(Token::Star),
                    "->" => Some(Token::Rstab),
                    "?" => Some(Token::Question),
                    "{-#" => Some(Token::FileMetadataStart),
                    "#-}" => Some(Token::FileMetadataEnd),
                    "comment" => Some(Token::Comment),
                    "bare identifier" => Some(Token::BareIdent("")),
                    "'@'-identifier" => Some(Token::AtIdent("")),
                    "'#'-identifier" => Some(Token::HashIdent("")),
                    "'%'-identifier" => Some(Token::PercentIdent("")),
                    "'^'-identifier" => Some(Token::CaretIdent("")),
                    "'!'-identifier" => Some(Token::BangIdent("")),
                    "string" => Some(Token::String("")),
                    "integer" => Some(Token::Int("")),
                    "hex-encoded integer" => Some(Token::Hex("")),
                    "binary-encoded integer" => Some(Token::Binary("")),
                    "end of input" => Some(Token::Eof),
                    _ => None,
                }
            }
            // We matched a keyword
            token => Some(token),
        }
    }
}
