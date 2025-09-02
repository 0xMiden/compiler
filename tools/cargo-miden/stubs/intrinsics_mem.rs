/// Unreachable stubs for intrinsics::mem interface

#[export_name = "intrinsics::mem::heap_base"]
pub extern "C" fn heap_base_stub() -> *mut u8 {
    unsafe { core::hint::unreachable_unchecked() }
}

