use core::ffi::c_void;

#[unsafe(export_name = "miden::protocol::tx::get_block_number")]
pub extern "C" fn tx_get_block_number_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_block_commitment")]
pub extern "C" fn tx_get_block_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_block_timestamp")]
pub extern "C" fn tx_get_block_timestamp_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_input_notes_commitment")]
pub extern "C" fn tx_get_input_notes_commitment_plain(_out: *mut core::ffi::c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_output_notes_commitment")]
pub extern "C" fn tx_get_output_notes_commitment_plain(_out: *mut core::ffi::c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_num_input_notes")]
pub extern "C" fn tx_get_num_input_notes_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_num_output_notes")]
pub extern "C" fn tx_get_num_output_notes_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_expiration_block_delta")]
pub extern "C" fn tx_get_expiration_block_delta_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::update_expiration_block_delta")]
pub extern "C" fn tx_update_expiration_block_delta_plain(_delta: f32) {
    unsafe { core::hint::unreachable_unchecked() }
}
