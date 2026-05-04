/// Unreachable stubs for intrinsics::debug interface

#[unsafe(export_name = "intrinsics::debug::break")]
pub extern "C" fn debug_break_stub() {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::debug::println")]
pub extern "C" fn debug_println_stub(_ptr: *const u8, _len: usize) {
    unsafe { core::hint::unreachable_unchecked() }
}
