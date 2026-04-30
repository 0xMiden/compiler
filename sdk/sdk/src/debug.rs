#[macro_export]
macro_rules! println {
    ($message:literal) => {{
        $crate::debug::println($message);
    }};
    ($message:expr) => {{
        $crate::debug::println($message);
    }};
}

#[inline(always)]
pub fn println(s: &str) {
    let bytes = s.as_bytes();
    miden_stdlib_sys::intrinsics::debug::println(bytes.as_ptr(), bytes.len());
}
