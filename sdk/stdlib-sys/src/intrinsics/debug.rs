#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
    #[link_name = "intrinsics::debug::break"]
    fn extern_break();
}

/// Sets a breakpoint in the emitted Miden Assembly at the point this function is called.
#[inline(always)]
#[track_caller]
#[cfg(all(target_family = "wasm", miden))]
pub fn breakpoint() {
    unsafe {
        extern_break();
    }
}

/// Sets a breakpoint in the emitted Miden Assembly at the point this function is called.
#[inline(always)]
#[track_caller]
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn breakpoint() {
    unimplemented!("debug intrinsics are only available when targeting the Miden VM")
}
