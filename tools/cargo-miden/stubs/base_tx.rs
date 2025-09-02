use core::ffi::c_void;

/// Tx interface stubs

#[export_name = "miden::tx::create_note"]
pub extern "C" fn tx_create_note_plain(
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

#[export_name = "miden::tx::add_asset_to_note"]
pub extern "C" fn tx_add_asset_to_note_plain(
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _note_idx: f32,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

