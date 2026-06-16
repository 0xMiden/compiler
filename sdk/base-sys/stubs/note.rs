use core::ffi::c_void;

/// Note interface stubs.
#[unsafe(export_name = "miden::protocol::note::compute_and_store_recipient")]
#[optimize(none)]
#[inline(never)]
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

#[unsafe(export_name = "miden::protocol::note::compute_storage_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_compute_storage_commitment_plain(
    _storage_ptr: *const c_void,
    _num_storage_items: usize,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::write_attachment_commitments_to_memory")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_write_attachment_commitments_to_memory_plain(
    _attachments_commitment_f0: f32,
    _attachments_commitment_f1: f32,
    _attachments_commitment_f2: f32,
    _attachments_commitment_f3: f32,
    _dest_ptr: *mut c_void,
) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::write_attachment_to_memory")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_write_attachment_to_memory_plain(
    _attachment_commitment_f0: f32,
    _attachment_commitment_f1: f32,
    _attachment_commitment_f2: f32,
    _attachment_commitment_f3: f32,
    _dest_ptr: *mut c_void,
) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::write_indexed_attachment_to_memory")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_write_indexed_attachment_to_memory_plain(
    _num_attachments: f32,
    _attachment_commitments_ptr: *const c_void,
    _attachment_idx: f32,
    _dest_ptr: *mut c_void,
) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::compute_recipient")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_compute_recipient_plain(
    _serial_num_f0: f32,
    _serial_num_f1: f32,
    _serial_num_f2: f32,
    _serial_num_f3: f32,
    _script_root_f0: f32,
    _script_root_f1: f32,
    _script_root_f2: f32,
    _script_root_f3: f32,
    _storage_commitment_f0: f32,
    _storage_commitment_f1: f32,
    _storage_commitment_f2: f32,
    _storage_commitment_f3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::metadata_into_sender")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_metadata_into_sender_plain(
    _metadata_f0: f32,
    _metadata_f1: f32,
    _metadata_f2: f32,
    _metadata_f3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::metadata_into_attachment_schemes")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_metadata_into_attachment_schemes_plain(
    _metadata_f0: f32,
    _metadata_f1: f32,
    _metadata_f2: f32,
    _metadata_f3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::metadata_into_note_type")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_metadata_into_note_type_plain(
    _metadata_f0: f32,
    _metadata_f1: f32,
    _metadata_f2: f32,
    _metadata_f3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::metadata_into_tag")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_metadata_into_tag_plain(
    _metadata_f0: f32,
    _metadata_f1: f32,
    _metadata_f2: f32,
    _metadata_f3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::note::find_attachment_idx")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn note_find_attachment_idx_plain(
    _attachment_scheme: f32,
    _metadata_f0: f32,
    _metadata_f1: f32,
    _metadata_f2: f32,
    _metadata_f3: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
