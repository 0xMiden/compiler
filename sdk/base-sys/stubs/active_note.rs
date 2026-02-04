use core::ffi::c_void;

/// Note interface stubs
#[unsafe(export_name = "miden::protocol::active_note::get_inputs")]
pub extern "C" fn note_get_inputs_plain(_ptr: *mut c_void) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_assets")]
pub extern "C" fn note_get_assets_plain(_ptr: *mut c_void) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_sender")]
pub extern "C" fn note_get_sender_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_recipient")]
pub extern "C" fn note_get_recipient_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_script_root")]
pub extern "C" fn note_get_script_root_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_serial_number")]
pub extern "C" fn note_get_serial_number_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_metadata")]
pub extern "C" fn note_get_metadata_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}
