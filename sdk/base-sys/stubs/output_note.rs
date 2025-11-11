/// Output note interface stubs
#[export_name = "miden::output_note::create"]
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

#[export_name = "miden::output_note::add_asset"]
pub extern "C" fn output_note_add_asset_plain(
    _a0: f32,
    _a1: f32,
    _a2: f32,
    _a3: f32,
    _note_idx: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
