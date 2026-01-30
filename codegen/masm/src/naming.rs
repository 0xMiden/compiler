use alloc::borrow::Cow;

/// Replaces `::` with `__` in `name`.
///
/// Miden Assembly uses `::` as a namespace separator in module/procedure paths. When Rust (or
/// compiler-generated) symbol names contain `::` as part of a single procedure name, they can be
/// misinterpreted as additional path components during linking. Converting `::` to `__` keeps such
/// names as a single procedure identifier.
pub(crate) fn double_colon_to_double_underscore(name: &str) -> Cow<'_, str> {
    if name.contains("::") {
        Cow::Owned(name.replace("::", "__"))
    } else {
        Cow::Borrowed(name)
    }
}
