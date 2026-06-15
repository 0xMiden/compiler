use core::ffi::c_void;

/// Output note interface stubs
#[unsafe(export_name = "miden::protocol::output_note::create")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_create_plain(
    _tag: f32,
    _note_type: f32,
    _r0: f32,
    _r1: f32,
    _r2: f32,
    _r3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::add_asset")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_add_asset_plain(
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _v0: f32,
    _v1: f32,
    _v2: f32,
    _v3: f32,
    _note_idx: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_assets_info")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_get_assets_info_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_assets")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_get_assets_plain(_dest_ptr: *mut c_void, _note_index: f32) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_attachments_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_get_attachments_commitment_plain(
    _note_index: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_recipient")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_get_recipient_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::get_metadata")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_get_metadata_plain(_note_index: f32, _out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::add_word_attachment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_add_word_attachment_plain(
    _attachment_scheme: f32,
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _note_index: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::add_attachment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_add_attachment_plain(
    _attachment_scheme: f32,
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _note_index: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::add_attachment_from_memory")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_add_attachment_from_memory_plain(
    _attachment_scheme: f32,
    _num_words: usize,
    _attachment_ptr: *const c_void,
    _note_index: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::find_attachment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_find_attachment_plain(
    _attachment_scheme: f32,
    _note_index: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::write_attachment_commitments_to_memory")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_write_attachment_commitments_to_memory_plain(
    _dest_ptr: *mut c_void,
    _note_index: f32,
) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::output_note::write_attachment_to_memory")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn output_note_write_attachment_to_memory_plain(
    _dest_ptr: *mut c_void,
    _attachment_idx: f32,
    _note_index: f32,
) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}
