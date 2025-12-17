/// Unreachable stubs for intrinsics::debug interface

#[unsafe(export_name = "intrinsics::debug::break")]
pub extern "C" fn debug_break_stub() {
    unsafe { core::hint::unreachable_unchecked() }
}
