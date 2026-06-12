use miden_stdlib_sys::{Felt, Word};

use super::types::AccountId;

/// Marker trait for raw FPI input array lengths supported by the protocol executor.
#[doc(hidden)]
pub trait SupportedForeignProcedureInputLen {}

macro_rules! supported_foreign_procedure_input_len {
    ($($len:expr),* $(,)?) => {
        $(
            impl SupportedForeignProcedureInputLen for [(); $len] {}
        )*
    };
}

supported_foreign_procedure_input_len!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);

/// Fully-padded input felts accepted by `execute_foreign_procedure`.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ForeignProcedureInputs {
    words: [Word; 4],
}

impl ForeignProcedureInputs {
    /// Creates raw FPI inputs and zero-pads unused protocol input slots.
    ///
    /// This is only implemented for input arrays with at most 16 felts.
    pub fn new<const N: usize>(values: [Felt; N]) -> Self
    where
        [(); N]: SupportedForeignProcedureInputLen,
    {
        let mut padded = [Felt::ZERO; 16];
        padded[..N].copy_from_slice(&values);

        Self {
            words: [
                Word::new([padded[3], padded[2], padded[1], padded[0]]),
                Word::new([padded[7], padded[6], padded[5], padded[4]]),
                Word::new([padded[11], padded[10], padded[9], padded[8]]),
                Word::new([padded[15], padded[14], padded[13], padded[12]]),
            ],
        }
    }
}

/// Fully-padded output felts returned by `execute_foreign_procedure`.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ForeignProcedureOutputs {
    words: [Word; 4],
}

impl ForeignProcedureOutputs {
    /// Returns the output felt at `index`.
    ///
    /// # Panics
    ///
    /// Panics if `index` is greater than or equal to 16.
    pub fn get(&self, index: usize) -> Felt {
        self.words[index / 4][3 - (index % 4)]
    }
}

/// Canonical raw FPI argument tuple consumed by the compiler's indirect lowering.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ForeignProcedureInvocation {
    /// Packed flattened FPI arguments: account id, procedure root, and 16 input felts.
    pub words: [Word; 6],
}

impl ForeignProcedureInvocation {
    /// Creates a raw FPI invocation tuple from SDK account and procedure values.
    pub fn new(
        foreign_account_id: AccountId,
        foreign_proc_root: Word,
        inputs: ForeignProcedureInputs,
    ) -> Self {
        let zero = Felt::ZERO;
        Self {
            words: [
                Word::new([
                    foreign_account_id.prefix,
                    foreign_account_id.suffix,
                    foreign_proc_root[0],
                    foreign_proc_root[1],
                ]),
                Word::new([
                    foreign_proc_root[2],
                    foreign_proc_root[3],
                    inputs.words[0][0],
                    inputs.words[0][1],
                ]),
                Word::new([
                    inputs.words[0][2],
                    inputs.words[0][3],
                    inputs.words[1][0],
                    inputs.words[1][1],
                ]),
                Word::new([
                    inputs.words[1][2],
                    inputs.words[1][3],
                    inputs.words[2][0],
                    inputs.words[2][1],
                ]),
                Word::new([
                    inputs.words[2][2],
                    inputs.words[2][3],
                    inputs.words[3][0],
                    inputs.words[3][1],
                ]),
                Word::new([inputs.words[3][2], inputs.words[3][3], zero, zero]),
            ],
        }
    }
}

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_block_number"]
    pub fn extern_tx_get_block_number() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_block_commitment"]
    pub fn extern_tx_get_block_commitment(ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_block_timestamp"]
    pub fn extern_tx_get_block_timestamp() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_input_notes_commitment"]
    pub fn extern_tx_get_input_notes_commitment(ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_output_notes_commitment"]
    pub fn extern_tx_get_output_notes_commitment(ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_num_input_notes"]
    pub fn extern_tx_get_num_input_notes() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_num_output_notes"]
    pub fn extern_tx_get_num_output_notes() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_expiration_block_delta"]
    pub fn extern_tx_get_expiration_block_delta() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::update_expiration_block_delta"]
    pub fn extern_tx_update_expiration_block_delta(delta: Felt);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::execute_foreign_procedure_indirect"]
    pub fn extern_tx_execute_foreign_procedure(
        invocation: *const ForeignProcedureInvocation,
        ptr: *mut ForeignProcedureOutputs,
    );
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
        ret_area.assume_init()
    }
}

/// Returns the block commitment of the reference block.
pub fn get_block_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_tx_get_block_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init()
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
        ret_area.assume_init()
    }
}

/// Executes `foreign_proc_root` against `foreign_account_id` with raw felt inputs.
///
/// The protocol executor always consumes exactly 16 input felts and returns exactly 16 output
/// felts. Callers whose target procedure uses fewer values can pass the actual values to
/// [`ForeignProcedureInputs::new`], which pads the remaining input slots with zeroes. Callers whose
/// target procedure returns fewer values should ignore the unused padded outputs.
///
/// # Panics
///
/// Propagates kernel errors if the foreign account ID is invalid, the foreign account inputs are
/// not available to the transaction, or the procedure root is not exported by the foreign account.
pub fn execute_foreign_procedure(
    foreign_account_id: AccountId,
    foreign_proc_root: Word,
    inputs: ForeignProcedureInputs,
) -> ForeignProcedureOutputs {
    unsafe {
        let invocation =
            ForeignProcedureInvocation::new(foreign_account_id, foreign_proc_root, inputs);
        let mut ret_area = ::core::mem::MaybeUninit::<ForeignProcedureOutputs>::uninit();
        extern_tx_execute_foreign_procedure(&invocation, ret_area.as_mut_ptr());
        ret_area.assume_init()
    }
}
