use core::ffi::c_void;

/// Note interface stubs.
#[unsafe(export_name = "miden::protocol::note::build_recipient")]
pub extern "C" fn note_build_recipient_plain(
    _storage_ptr: *mut c_void,
    _num_storage_items: usize,
    _serial_num_f0: f32,
    _serial_num_f1: f32,
    _serial_num_f2: f32,
    _serial_num_f3: f32,
    _script_root_f0: f32,
    _script_root_f1: f32,
    _script_root_f2: f32,
    _script_root_f3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
