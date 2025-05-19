#[link(wasm_import_module = "miden:core-import/intrinsics-debug@1.0.0")]
extern "C" {
    #[link_name = "break"]
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
