use core::ffi::c_void;

/// Output note interface stubs
#[unsafe(export_name = "miden::protocol::output_note::create")]
pub extern "C" fn output_note_create_plain(
    _tag: f32,
    _aux: f32,
    _note_type: f32,
    _execution_hint: f32,
    _r0: f32,
    _r1: f32,
    _r2: f32,
    _r3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::add_asset")]
pub extern "C" fn output_note_add_asset_plain(
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _note_idx: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_assets_info")]
pub extern "C" fn output_note_get_assets_info_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_assets")]
pub extern "C" fn output_note_get_assets_plain(_dest_ptr: *mut c_void, _note_index: f32) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_recipient")]
pub extern "C" fn output_note_get_recipient_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_metadata")]
pub extern "C" fn output_note_get_metadata_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}
