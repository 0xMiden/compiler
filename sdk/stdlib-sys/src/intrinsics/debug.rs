unsafe extern "C" {
    #[link_name = "intrinsics::debug::break"]
    fn extern_break();
}

/// Sets a breakpoint in the emitted Miden Assembly at the point this function is called.
#[inline(always)]
#[track_caller]
pub fn breakpoint() {
    unsafe {
        extern_break();
    }
}
