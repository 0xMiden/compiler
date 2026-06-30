#[macro_export]
macro_rules! println {
    ($message:literal) => {{
        $crate::debug::println($message);
    }};
    ($message:expr) => {{
        $crate::debug::println($message);
    }};
    ($format:literal, $($arg:tt),+) => {
        compile_error!("unsupported use of println! with format arguments");
    };
}

#[inline(always)]
pub fn println(s: &str) {
    let bytes = s.as_bytes();
    miden_stdlib_sys::intrinsics::debug::println(bytes.as_ptr(), bytes.len());
}
