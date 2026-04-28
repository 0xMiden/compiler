use core::ffi::c_void;

/// Input note interface stubs.
define_stub! {
    #[unsafe(export_name = "miden::protocol::input_note::get_assets_info")]
    pub extern "C" fn input_note_get_assets_info_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::input_note::get_assets")]
    pub extern "C" fn input_note_get_assets_plain(
        dest_ptr: *mut c_void,
        note_index: f32,
    ) -> usize;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::input_note::get_recipient")]
    pub extern "C" fn input_note_get_recipient_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::input_note::get_metadata")]
    pub extern "C" fn input_note_get_metadata_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::input_note::get_sender")]
    pub extern "C" fn input_note_get_sender_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::input_note::get_storage_info")]
    pub extern "C" fn input_note_get_storage_info_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::input_note::get_script_root")]
    pub extern "C" fn input_note_get_script_root_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::input_note::get_serial_number")]
    pub extern "C" fn input_note_get_serial_number_plain(note_index: f32, out: *mut c_void);
}
