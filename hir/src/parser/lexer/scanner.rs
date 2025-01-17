use core::{
    iter::{FusedIterator, Peekable},
    ops::Range,
};

use crate::diagnostics::{ByteIndex, ByteOffset};

/// A simple raw character source for [super::Lexer];
pub struct Scanner<'a> {
    src: &'a str,
    buf: Peekable<core::str::CharIndices<'a>>,
    next: Option<(ByteIndex, char)>,
    pos: ByteIndex,
    eof: bool,
}
impl<'a> Scanner<'a> {
    pub fn new(src: &'a str) -> Self {
        let eof = src.is_empty();
        let mut buf = src.char_indices().peekable();
        let next = buf.next().map(|(i, c)| (ByteIndex::from(i as u32), c));
        Self {
            src,
            buf,
            next,
            pos: next.map(|(i, c)| i + ByteOffset::from_char_len(c)).unwrap_or_default(),
            eof,
        }
    }

    #[inline(always)]
    pub const fn read(&self) -> Option<(ByteIndex, char)> {
        self.next
    }

    pub fn peek(&mut self) -> Option<(ByteIndex, char)> {
        self.buf.peek().and_then(|&(i, c)| match u32::try_from(i) {
            Ok(i) => Some((ByteIndex::from(i), c)),
            Err(_) => None,
        })
    }

    #[inline]
    fn advance(&mut self) {
        match self.buf.next() {
            Some((i, c)) if i < u32::MAX as usize => {
                let i = ByteIndex::from(i as u32);
                self.pos = i + ByteOffset::from_char_len(c);
                self.next = Some((i, c));
            }
            Some(_) => {
                panic!("invalid source file: only files smaller than 2^32 bytes are supported")
            }
            None => {
                self.eof = true;
                self.next = None;
            }
        }
    }

    pub fn position(&self) -> ByteIndex {
        self.pos
    }

    #[inline(always)]
    pub fn slice(&self, span: impl Into<Range<usize>>) -> &str {
        &self.src[span.into()]
    }
}

impl Iterator for Scanner<'_> {
    type Item = (ByteIndex, char);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.eof {
            return None;
        }

        let current = self.next.take();
        self.advance();
        current
    }
}

impl FusedIterator for Scanner<'_> {}
