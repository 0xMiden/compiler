#[cfg(target_arch = "wasm32")]
unsafe extern "C" {
    #[link_name = "intrinsics::debug::break"]
    fn extern_break();
}

/// Sets a breakpoint in the emitted Miden Assembly at the point this function is called.
#[inline(always)]
#[track_caller]
#[cfg(target_arch = "wasm32")]
pub fn breakpoint() {
    unsafe {
        extern_break();
    }
}

/// Sets a breakpoint in the emitted Miden Assembly at the point this function is called.
#[inline(always)]
#[track_caller]
#[cfg(not(target_arch = "wasm32"))]
pub fn breakpoint() {
    unimplemented!("debug intrinsics are only available on wasm32")
}
