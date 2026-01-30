use alloc::borrow::Cow;

/// Sanitizes a procedure name for use in Miden Assembly.
///
/// Miden Assembly uses `::` as a namespace separator in module/procedure paths. When Rust (or
/// compiler-generated) symbol names contain `::` as part of a single procedure name, they can be
/// misinterpreted as additional path components during linking. Converting `::` to `__` keeps such
/// names as a single procedure identifier.
///
/// In addition, some symbol names (e.g. those derived from Wasm component model identifiers) may
/// contain characters which are not valid in a MASM procedure identifier (such as `-`, `#`, or
/// `/`). These are rewritten to `_` to ensure the resulting procedure path is valid.
pub(crate) fn sanitize_procedure_name(name: &str) -> Cow<'_, str> {
    fn is_valid_start(ch: u8) -> bool {
        ch == b'_' || ch.is_ascii_alphabetic()
    }
    fn is_valid_part(ch: u8) -> bool {
        ch == b'_' || ch.is_ascii_alphanumeric()
    }

    let bytes = name.as_bytes();
    let already_valid = !name.contains("::")
        && !bytes.is_empty()
        && is_valid_start(bytes[0])
        && bytes.iter().all(|b| is_valid_part(*b));
    if already_valid {
        return Cow::Borrowed(name);
    }

    // Fast path for empty names: use a stable placeholder.
    if name.is_empty() {
        return Cow::Borrowed("_");
    }

    let mut out = alloc::string::String::with_capacity(name.len());
    let mut iter = name.chars().peekable();
    while let Some(ch) = iter.next() {
        match ch {
            ':' if matches!(iter.peek(), Some(':')) => {
                // Rewrite `::` so it's not interpreted as an additional namespace separator.
                iter.next();
                out.push('_');
                out.push('_');
            }
            ch if ch.is_ascii_alphanumeric() || ch == '_' => out.push(ch),
            _ => out.push('_'),
        }
    }

    // Ensure the first character is valid for a MASM identifier.
    let first = out.as_bytes().first().copied().unwrap_or(b'_');
    if !is_valid_start(first) {
        out.insert(0, '_');
    }

    Cow::Owned(out)
}
