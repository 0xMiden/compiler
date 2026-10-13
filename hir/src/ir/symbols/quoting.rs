//! The quoted spelling of a HIR symbol name shared by the HIR printer and the lexer: `"..."`, with
//! `"` and `\` escaped by a `\`.

use alloc::string::String;

/// Appends `name` to `out` quoted, `"..."`, with `"` and `\` escaped by a `\`.
pub(crate) fn push_quoted(out: &mut String, name: &str) {
    out.reserve(name.len() + 2);
    out.push('"');
    for c in name.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

/// Reads the quoted name whose opening `"` precedes `quoted`, the inverse of the quoting the HIR
/// printer applies.
///
/// A `\` takes the character after it literally. Returns the unescaped name and the text after
/// its closing `"`, or `None` when the name does not close.
pub fn unescape_quoted(quoted: &str) -> Option<(String, &str)> {
    let mut name = String::new();
    let mut chars = quoted.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((name, &quoted[at + 1..])),
            '\\' => name.push(chars.next()?.1),
            c => name.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_quoted_inverts_push_quoted() {
        for name in ["", "a", r#"a"b"#, r#"a\b"#, r#"\"#, "a::b"] {
            let mut quoted = String::new();
            push_quoted(&mut quoted, name);
            quoted.push_str("::rest");
            let (unescaped, rest) = unescape_quoted(&quoted[1..]).expect("a closed name");
            assert_eq!(unescaped, name);
            assert_eq!(rest, "::rest");
        }
        assert_eq!(unescape_quoted(r#"abc"#), None);
        assert_eq!(unescape_quoted(r#"abc\"#), None);
    }
}
