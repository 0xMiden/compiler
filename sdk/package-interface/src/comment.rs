//! Text from a package, made safe to write into a generated one-line comment.

use alloc::string::String;

/// `text` with every character a generated WIT or Rust line comment cannot carry replaced by a
/// space, so that it stays inside the one-line comment it is written to and the document still
/// parses.
///
/// The replaced characters are the control characters (line breaks included), the bidirectional
/// formatting characters U+202A–U+202E and U+2066–U+2069 (which wit-parser rejects anywhere and
/// rustc denies in comments), and the deprecated characters wit-parser rejects (U+0149, U+0673,
/// U+0F77, U+0F79, U+17A3, U+17A4, U+17B4, U+17B5). Every other character, non-ASCII ones
/// included, is kept, so names stay readable.
pub fn one_line(text: &str) -> String {
    text.chars().map(|c| if is_forbidden(c) { ' ' } else { c }).collect()
}

/// Whether a generated comment must not carry `c`.
fn is_forbidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{149}'
                | '\u{673}'
                | '\u{f77}'
                | '\u{f79}'
                | '\u{17a3}'
                | '\u{17a4}'
                | '\u{17b4}'
                | '\u{17b5}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Line breaks, bidirectional overrides and deprecated characters become spaces; other
    /// non-ASCII text is kept.
    #[test]
    fn forbidden_characters_become_spaces() {
        assert_eq!(one_line("a\nb\r\tc"), "a b  c");
        assert_eq!(one_line("x\u{202e}y\u{2066}z\u{149}"), "x y z ");
        assert_eq!(one_line("café::ñame"), "café::ñame");
    }
}
