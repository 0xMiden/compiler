use core::ffi::c_void;

/// Output note interface stubs.
define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::create")]
    pub extern "C" fn output_note_create_plain(
        tag: f32,
        note_type: f32,
        r0: f32,
        r1: f32,
        r2: f32,
        r3: f32,
    ) -> f32;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::add_asset")]
    pub extern "C" fn output_note_add_asset_plain(
        k0: f32,
        k1: f32,
        k2: f32,
        k3: f32,
        v0: f32,
        v1: f32,
        v2: f32,
        v3: f32,
        note_idx: f32,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::get_assets_info")]
    pub extern "C" fn output_note_get_assets_info_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::get_assets")]
    pub extern "C" fn output_note_get_assets_plain(
        dest_ptr: *mut c_void,
        note_index: f32,
    ) -> usize;
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::get_recipient")]
    pub extern "C" fn output_note_get_recipient_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::get_metadata")]
    pub extern "C" fn output_note_get_metadata_plain(note_index: f32, out: *mut c_void);
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::set_attachment")]
    pub extern "C" fn output_note_set_attachment_plain(
        note_index: f32,
        attachment_scheme: f32,
        attachment_kind: f32,
        a0: f32,
        a1: f32,
        a2: f32,
        a3: f32,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::set_word_attachment")]
    pub extern "C" fn output_note_set_word_attachment_plain(
        note_index: f32,
        attachment_scheme: f32,
        a0: f32,
        a1: f32,
        a2: f32,
        a3: f32,
    );
}

define_stub! {
    #[unsafe(export_name = "miden::protocol::output_note::set_array_attachment")]
    pub extern "C" fn output_note_set_array_attachment_plain(
        note_index: f32,
        attachment_scheme: f32,
        a0: f32,
        a1: f32,
        a2: f32,
        a3: f32,
    );
}
