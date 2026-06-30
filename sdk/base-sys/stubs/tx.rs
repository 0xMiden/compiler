use core::ffi::c_void;

#[unsafe(export_name = "miden::protocol::tx::get_block_number")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_block_number_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_block_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_block_commitment_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_block_timestamp")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_block_timestamp_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_input_notes_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_input_notes_commitment_plain(_out: *mut core::ffi::c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_output_notes_commitment")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_output_notes_commitment_plain(_out: *mut core::ffi::c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_num_input_notes")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_num_input_notes_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_num_output_notes")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_num_output_notes_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_expiration_block_delta")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_expiration_block_delta_plain() -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::update_expiration_block_delta")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_update_expiration_block_delta_plain(_delta: f32) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::get_tx_script_root")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_get_tx_script_root_plain(_out: *mut c_void) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "miden::protocol::tx::execute_foreign_procedure_indirect")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn tx_execute_foreign_procedure_indirect(
    _invocation: *const c_void,
    _out: *mut c_void,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
