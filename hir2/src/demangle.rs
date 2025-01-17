/// Demangle `name`, where `name` was mangled using Rust's mangling scheme
#[inline]
pub fn demangle<S: AsRef<str>>(name: S) -> String {
    demangle_impl(name.as_ref())
}

fn demangle_impl(name: &str) -> String {
    let mut input = name.as_bytes();
    let mut demangled = Vec::with_capacity(input.len() * 2);
    rustc_demangle::demangle_stream(&mut input, &mut demangled, /* include_hash= */ false)
        .expect("failed to write demangled identifier");
    String::from_utf8(demangled).expect("demangled identifier contains invalid utf-8")
}
