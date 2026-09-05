/// Prints a message through Miden's trace-based `PrintLn` functionality.
///
/// # Formatting
///
/// The formatting variant requires the `alloc` crate and a configured global allocator.
///
/// This supports most common use cases of Rust's `format!`. However, named arguments without
/// argument list are not supported, for example `println!("result: {res}");`
#[macro_export]
macro_rules! println {
    ($message:literal) => {{
        $crate::debug::println($message);
    }};
    ($message:expr) => {{
        $crate::debug::println($message);
    }};
    ($format:literal, $($arg:tt)+) => {{
        let message = ::alloc::format!($format, $($arg)+);
        $crate::debug::println(&message);
    }};
}

#[inline(always)]
pub fn println(s: &str) {
    let bytes = s.as_bytes();
    miden_stdlib_sys::intrinsics::debug::println(bytes.as_ptr(), bytes.len());
}
