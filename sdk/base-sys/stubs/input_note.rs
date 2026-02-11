use core::ffi::c_void;

/// Input note interface stubs
#[unsafe(export_name = "miden::protocol::input_note::get_assets_info")]
pub extern "C" fn input_note_get_assets_info_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_assets")]
pub extern "C" fn input_note_get_assets_plain(_dest_ptr: *mut c_void, _note_index: f32) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_recipient")]
pub extern "C" fn input_note_get_recipient_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_metadata")]
pub extern "C" fn input_note_get_metadata_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_sender")]
pub extern "C" fn input_note_get_sender_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_inputs_info")]
pub extern "C" fn input_note_get_inputs_info_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_script_root")]
pub extern "C" fn input_note_get_script_root_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_serial_number")]
pub extern "C" fn input_note_get_serial_number_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}
