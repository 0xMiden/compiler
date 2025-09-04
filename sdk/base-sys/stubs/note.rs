use core::ffi::c_void;

/// Note interface stubs
#[export_name = "miden::note::get_inputs"]
pub extern "C" fn note_get_inputs_plain(_ptr: *mut c_void) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::note::get_assets"]
pub extern "C" fn note_get_assets_plain(_ptr: *mut c_void) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

