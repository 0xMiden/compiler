use super::Token;

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum Delimiter {
    /// Zero or more operands with no delimiters.
    #[default]
    None,
    /// Parens surrounding zero or more operands.
    Paren,
    /// Square brackets surrounding zero or more operands.
    Bracket,
    /// <> brackets surrounding zero or more operands.
    Angle,
    /// {} brackets surrounding zero or more operands.
    Brace,
    /// Parens supporting zero or more operands, or nothing.
    OptionalParen,
    /// Square brackets supporting zero or more ops, or nothing.
    OptionalBracket,
    /// <> brackets supporting zero or more ops, or nothing.
    OptionalAngle,
    /// {} brackets surrounding zero or more operands, or nothing.
    OptionalBrace,
}

impl Delimiter {
    /// Returns true if this delimiter is required
    pub fn is_optional(&self) -> bool {
        matches!(
            self,
            Self::OptionalParen | Self::OptionalBracket | Self::OptionalAngle | Self::OptionalBrace
        )
    }

    /// Returns true if `tok` is the opening token type of this delimiter
    pub fn is_open(&self, tok: &Token<'_>) -> bool {
        self.open().is_some_and(|open| open.eq(tok))
    }

    /// Returns true if `c` is the closing character of this delimiter
    pub fn is_close(&self, tok: &Token<'_>) -> bool {
        self.close().is_some_and(|close| close.eq(tok))
    }

    /// Returns the character which opens this delimiter, if applicable
    pub fn open(&self) -> Option<Token<'static>> {
        Some(match self {
            Self::None => return None,
            Self::Paren | Self::OptionalParen => Token::Lparen,
            Self::Bracket | Self::OptionalBracket => Token::Lbracket,
            Self::Angle | Self::OptionalAngle => Token::Langle,
            Self::Brace | Self::OptionalBrace => Token::Lbrace,
        })
    }

    /// Returns the character which closes this delimiter, if applicable
    pub fn close(&self) -> Option<Token<'static>> {
        Some(match self {
            Self::None => return None,
            Self::Paren | Self::OptionalParen => Token::Rparen,
            Self::Bracket | Self::OptionalBracket => Token::Rbracket,
            Self::Angle | Self::OptionalAngle => Token::Rangle,
            Self::Brace | Self::OptionalBrace => Token::Rbrace,
        })
    }

    /// Returns a string which can be used in `expected '('`-like messages
    pub fn expected_open(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::Paren | Self::OptionalParen => "'('",
            Self::Bracket | Self::OptionalBracket => "'['",
            Self::Angle | Self::OptionalAngle => "'<'",
            Self::Brace | Self::OptionalBrace => "'{'",
        }
    }

    /// Returns a string which can be used in `expected ')'`-like messages
    pub fn expected_close(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::Paren | Self::OptionalParen => "')'",
            Self::Bracket | Self::OptionalBracket => "']'",
            Self::Angle | Self::OptionalAngle => "'>'",
            Self::Brace | Self::OptionalBrace => "'}'",
        }
    }
}
