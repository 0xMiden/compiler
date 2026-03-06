use miden_stdlib_sys::{Felt, Word};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::tx::get_block_number"]
    pub fn extern_tx_get_block_number() -> Felt;

    #[link_name = "miden::protocol::tx::get_block_commitment"]
    pub fn extern_tx_get_block_commitment(ptr: *mut Word);

    #[link_name = "miden::protocol::tx::get_block_timestamp"]
    pub fn extern_tx_get_block_timestamp() -> Felt;

    #[link_name = "miden::protocol::tx::get_input_notes_commitment"]
    pub fn extern_tx_get_input_notes_commitment(ptr: *mut Word);

    #[link_name = "miden::protocol::tx::get_output_notes_commitment"]
    pub fn extern_tx_get_output_notes_commitment(ptr: *mut Word);

    #[link_name = "miden::protocol::tx::get_num_input_notes"]
    pub fn extern_tx_get_num_input_notes() -> Felt;

    #[link_name = "miden::protocol::tx::get_num_output_notes"]
    pub fn extern_tx_get_num_output_notes() -> Felt;

    #[link_name = "miden::protocol::tx::get_expiration_block_delta"]
    pub fn extern_tx_get_expiration_block_delta() -> Felt;

    #[link_name = "miden::protocol::tx::update_expiration_block_delta"]
    pub fn extern_tx_update_expiration_block_delta(delta: Felt);
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
        ret_area.assume_init().reversed()
    }
}

/// Returns the block commitment of the reference block.
pub fn get_block_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_tx_get_block_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns the timestamp of the reference block.
pub fn get_block_timestamp() -> Felt {
    unsafe { extern_tx_get_block_timestamp() }
}

/// Returns the total number of input notes consumed by the transaction.
pub fn get_num_input_notes() -> Felt {
    unsafe { extern_tx_get_num_input_notes() }
}

/// Returns the number of output notes created so far in the transaction.
pub fn get_num_output_notes() -> Felt {
    unsafe { extern_tx_get_num_output_notes() }
}

/// Returns the transaction expiration block delta.
pub fn get_expiration_block_delta() -> Felt {
    unsafe { extern_tx_get_expiration_block_delta() }
}

/// Updates the transaction expiration block delta.
pub fn update_expiration_block_delta(delta: Felt) {
    unsafe {
        extern_tx_update_expiration_block_delta(delta);
    }
}

/// Returns the output notes commitment digest.
pub fn get_output_notes_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_tx_get_output_notes_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}
