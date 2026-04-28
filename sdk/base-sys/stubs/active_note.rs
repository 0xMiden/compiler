use core::ffi::c_void;

/// Note interface stubs.
///
/// In protocol v0.14, note "inputs" are exposed via `active_note::get_storage`.
define_stub! {
    #[unsafe(export_name = "miden::protocol::active_note::get_storage")]
    pub extern "C" fn note_get_inputs_plain(ptr: *mut c_void) -> usize;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_note::get_assets")]
    pub extern "C" fn note_get_assets_plain(ptr: *mut c_void) -> usize;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_note::get_sender")]
    pub extern "C" fn note_get_sender_plain(ptr: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_note::get_recipient")]
    pub extern "C" fn note_get_recipient_plain(ptr: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_note::get_script_root")]
    pub extern "C" fn note_get_script_root_plain(ptr: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_note::get_serial_number")]
    pub extern "C" fn note_get_serial_number_plain(ptr: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::active_note::get_metadata")]
    pub extern "C" fn note_get_metadata_plain(ptr: *mut c_void);
}
