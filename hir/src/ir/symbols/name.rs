/// Represents the name of a [Symbol] in its local [SymbolTable]
pub type SymbolName = crate::interner::Symbol;

/// Generate a unique symbol name.
///
/// Iteratively increase `counter` and use it as a suffix for symbol names until `is_unique` does
/// not detect any conflict.
pub fn generate_symbol_name<F>(name: SymbolName, counter: &mut usize, is_unique: F) -> SymbolName
where
    F: Fn(&str) -> bool,
{
    use core::fmt::Write;

    use crate::SmallStr;

    if is_unique(name.as_str()) {
        return name;
    }

    let base_len = name.as_str().len();
    let mut buf = SmallStr::with_capacity(base_len + 2);
    buf.push_str(name.as_str());
    loop {
        *counter += 1;
        buf.truncate(base_len);
        buf.push('_');
        write!(&mut buf, "{counter}").unwrap();

        if is_unique(buf.as_str()) {
            break SymbolName::intern(buf);
        }
    }
}
