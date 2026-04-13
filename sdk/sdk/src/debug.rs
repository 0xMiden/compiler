#[inline(always)]
pub fn println(s: &str) {
    let bytes = s.as_bytes();
    miden_stdlib_sys::intrinsics::debug::println(bytes.as_ptr(), bytes.len());
}
