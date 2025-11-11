use core::ffi::c_void;

#[export_name = "miden::tx::get_block_number"]
pub extern "C" fn tx_get_block_number_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::tx::get_input_notes_commitment"]
pub extern "C" fn tx_get_input_notes_commitment_plain(_out: *mut core::ffi::c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "miden::tx::get_output_notes_commitment"]
pub extern "C" fn tx_get_output_notes_commitment_plain(_out: *mut core::ffi::c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}
