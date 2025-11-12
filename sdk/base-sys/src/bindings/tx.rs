use miden_stdlib_sys::{Felt, Word};

#[allow(improper_ctypes)]
extern "C" {
    #[link_name = "miden::tx::get_block_number"]
    pub fn extern_tx_get_block_number() -> Felt;

    #[link_name = "miden::tx::get_input_notes_commitment"]
    pub fn extern_tx_get_input_notes_commitment(ptr: *mut Word);

    #[link_name = "miden::tx::get_output_notes_commitment"]
    pub fn extern_tx_get_output_notes_commitment(ptr: *mut Word);
}

/// Returns the current block number.
pub fn get_block_number() -> Felt {
    unsafe { extern_tx_get_block_number() }
}

/// Returns the input notes commitment digest.
pub fn get_input_notes_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_tx_get_input_notes_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the output notes commitment digest.
pub fn get_output_notes_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_tx_get_output_notes_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}
