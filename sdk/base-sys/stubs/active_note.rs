use core::ffi::c_void;

/// Note interface stubs
// NOTE: In protocol v0.14, note "inputs" are exposed via `active_note::get_storage`.
#[unsafe(export_name = "miden::protocol::active_note::get_storage")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_get_inputs_plain(_ptr: *mut c_void) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_assets")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_get_assets_plain(_ptr: *mut c_void) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_sender")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_get_sender_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_recipient")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_get_recipient_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_script_root")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_get_script_root_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_serial_number")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_get_serial_number_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::active_note::get_metadata")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_get_metadata_plain(_ptr: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}
