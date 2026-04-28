use core::ffi::c_void;

/// Note interface stubs.
define_stub! {
    #[unsafe(export_name = "miden::protocol::note::build_recipient")]
    pub extern "C" fn note_build_recipient_plain(
        storage_ptr: *mut c_void,
        num_storage_items: usize,
        serial_num_f0: f32,
        serial_num_f1: f32,
        serial_num_f2: f32,
        serial_num_f3: f32,
        script_root_f0: f32,
        script_root_f1: f32,
        script_root_f2: f32,
        script_root_f3: f32,
        out: *mut c_void,
    );
}
