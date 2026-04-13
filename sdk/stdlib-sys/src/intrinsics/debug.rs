#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
    #[link_name = "intrinsics::debug::break"]
    fn extern_break();

    #[link_name = "intrinsics::debug::println"]
    fn extern_println(ptr: *const u8, len: usize);
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

/// Prints the string pointed to by `ptr` in the debug executor.
#[inline(always)]
#[cfg(all(target_family = "wasm", miden))]
pub fn println(ptr: *const u8, len: usize) {
    unsafe {
        extern_println(ptr, len);
    }
}

/// Sets a breakpoint in the emitted Miden Assembly at the point this function is called.
#[inline(always)]
#[track_caller]
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn breakpoint() {
    unimplemented!("debug intrinsics are only available when targeting the Miden VM")
}

/// Prints the string pointed to by `ptr` in the debug executor.
#[inline(always)]
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn println(_ptr: *const u8, _len: usize) {
    unimplemented!("debug intrinsics are only available when targeting the Miden VM")
}
