use core::ffi::c_void;

/// Input note interface stubs
#[unsafe(export_name = "miden::protocol::input_note::get_initial_assets_info")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_initial_assets_info_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_initial_assets")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_initial_assets_plain(
    _dest_ptr: *mut c_void,
    _note_index: f32,
) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_recipient")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_recipient_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_metadata")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_metadata_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_sender")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_sender_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_storage_info")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_storage_info_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_script_root")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_script_root_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_serial_number")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_serial_number_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_attachments_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_attachments_commitment_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::write_attachment_commitments_to_memory")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_write_attachment_commitments_to_memory_plain(
    _dest_ptr: *mut c_void,
    _note_index: f32,
) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::write_attachment_to_memory")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_write_attachment_to_memory_plain(
    _dest_ptr: *mut c_void,
    _attachment_idx: f32,
    _note_index: f32,
) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::find_attachment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_find_attachment_plain(
    _attachment_scheme: f32,
    _note_index: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_initial_num_assets")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_initial_num_assets_plain(_note_index: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_asset")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_asset_plain(
    _asset_index: f32,
    _note_index: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::remove_asset")]
#[optimize(none)]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn input_note_remove_asset_plain(
    _asset_id_0: f32,
    _asset_id_1: f32,
    _asset_id_2: f32,
    _asset_id_3: f32,
    _asset_value_0: f32,
    _asset_value_1: f32,
    _asset_value_2: f32,
    _asset_value_3: f32,
    _note_index: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::get_note_id")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_get_note_id_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::input_note::find_note")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn input_note_find_note_plain(
    _note_id_0: f32,
    _note_id_1: f32,
    _note_id_2: f32,
    _note_id_3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
