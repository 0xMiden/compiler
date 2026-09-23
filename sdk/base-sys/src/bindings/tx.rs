use miden_stdlib_sys::{Felt, Word, WordAligned};

use super::types::{AccountId, AssetAmount, AssetId, BlockNumber};

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
///
/// Slot `i` is the `i`-th felt of the callee's `#! Inputs:` list, so slot `0` is on top of the
/// callee's stack. A `Word` passed as `[w[0], w[1], w[2], w[3]]` reaches the callee as the same
/// `Word`.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ForeignProcedureInputs {
    felts: [Felt; 16],
}

impl ForeignProcedureInputs {
    /// Creates raw FPI inputs where `values[i]` fills input slot `i`, and zero-pads the unused
    /// trailing slots.
    ///
    /// This is only implemented for input arrays with at most 16 felts.
    pub fn new<const N: usize>(values: [Felt; N]) -> Self
    where
        [(); N]: SupportedForeignProcedureInputLen,
    {
        let mut felts = [Felt::ZERO; 16];
        felts[..N].copy_from_slice(&values);
        Self { felts }
    }
}

/// Fully-padded output felts returned by `execute_foreign_procedure`.
///
/// Slot `i` is the `i`-th felt of the callee's `#! Outputs:` list, so slot `0` is on top of the
/// callee's stack on return. A `Word` the callee leaves on top reads back as
/// `[get(0), get(1), get(2), get(3)]`.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ForeignProcedureOutputs {
    // The compiler stores the 16 executor results consecutively, top of stack first.
    felts: [Felt; 16],
}

impl ForeignProcedureOutputs {
    /// Returns the output felt in slot `index`.
    ///
    /// # Panics
    ///
    /// Panics if `index` is greater than or equal to 16.
    pub fn get(&self, index: usize) -> Felt {
        self.felts[index]
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
                    inputs.felts[0],
                    inputs.felts[1],
                ]),
                Word::new([inputs.felts[2], inputs.felts[3], inputs.felts[4], inputs.felts[5]]),
                Word::new([inputs.felts[6], inputs.felts[7], inputs.felts[8], inputs.felts[9]]),
                Word::new([inputs.felts[10], inputs.felts[11], inputs.felts[12], inputs.felts[13]]),
                Word::new([inputs.felts[14], inputs.felts[15], zero, zero]),
            ],
        }
    }
}

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_reference_block_number"]
    pub fn extern_tx_get_reference_block_number() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_reference_block_commitment"]
    pub fn extern_tx_get_reference_block_commitment(ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_block_commitment"]
    pub fn extern_tx_get_block_commitment(block_number: Felt, ptr: *mut Word);
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
    #[link_name = "miden::protocol::tx::get_tx_script_root"]
    pub fn extern_tx_get_tx_script_root(ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::execute_foreign_procedure_indirect"]
    pub fn extern_tx_execute_foreign_procedure(
        invocation: *const ForeignProcedureInvocation,
        ptr: *mut ForeignProcedureOutputs,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::compute_fee"]
    fn extern_tx_compute_fee(
        num_extra_cycles: Felt,
        exclude_notes_commitment_0: Felt,
        exclude_notes_commitment_1: Felt,
        exclude_notes_commitment_2: Felt,
        exclude_notes_commitment_3: Felt,
    ) -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::tx::get_fee_asset_id"]
    fn extern_tx_get_fee_asset_id(ptr: *mut AssetId);
}

/// Returns the transaction reference block number.
pub fn get_reference_block_number() -> BlockNumber {
    BlockNumber {
        // The transaction kernel guarantees block numbers fit in a u32.
        inner: unsafe { extern_tx_get_reference_block_number() },
    }
}

/// Returns the input notes commitment digest.
pub fn get_input_notes_commitment() -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_tx_get_input_notes_commitment(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the block commitment of the reference block.
pub fn get_reference_block_commitment() -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_tx_get_reference_block_commitment(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the commitment of the block with the given number.
///
/// Any block up to and including the reference block can be read; the transaction kernel aborts
/// for later blocks.
pub fn get_block_commitment(block_number: BlockNumber) -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_tx_get_block_commitment(block_number.as_felt(), ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the timestamp of the reference block, in seconds.
pub fn get_block_timestamp() -> u32 {
    // The transaction kernel guarantees block timestamps fit in a u32.
    let timestamp = unsafe { extern_tx_get_block_timestamp() };
    timestamp.as_canonical_u64() as u32
}

/// Returns the total number of input notes consumed by the transaction.
pub fn get_num_input_notes() -> u32 {
    // The transaction kernel guarantees note counts fit in a u32.
    let count = unsafe { extern_tx_get_num_input_notes() };
    count.as_canonical_u64() as u32
}

/// Returns the number of output notes created so far in the transaction.
pub fn get_num_output_notes() -> u32 {
    // The transaction kernel guarantees note counts fit in a u32.
    let count = unsafe { extern_tx_get_num_output_notes() };
    count.as_canonical_u64() as u32
}

/// Returns the transaction expiration block delta, or `0` if no expiration delta has been set.
pub fn get_expiration_block_delta() -> u16 {
    // Set deltas are kernel-bounded to 1..=u16::MAX; the kernel returns 0 for an unset delta.
    let delta = unsafe { extern_tx_get_expiration_block_delta() };
    delta.as_canonical_u64() as u16
}

/// Updates the transaction expiration block delta.
///
/// The transaction kernel accepts deltas in `1..=u16::MAX`.
pub fn update_expiration_block_delta(delta: u16) {
    unsafe {
        extern_tx_update_expiration_block_delta(Felt::from(delta));
    }
}

/// Returns the transaction script root.
pub fn get_tx_script_root() -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_tx_get_tx_script_root(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the output notes commitment digest.
pub fn get_output_notes_commitment() -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_tx_get_output_notes_commitment(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Executes `foreign_proc_root` against `foreign_account_id` with raw felt inputs.
///
/// The protocol executor always consumes exactly 16 input felts and returns exactly 16 output
/// felts, both in the order of the callee's `#! Inputs:` and `#! Outputs:` lists. Callers whose
/// target procedure uses fewer values can pass the actual values to
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
        let mut ret_area =
            WordAligned::new(::core::mem::MaybeUninit::<ForeignProcedureOutputs>::uninit());
        extern_tx_execute_foreign_procedure(&invocation, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Computes the fee the current transaction owes, denominated in the fee asset.
///
/// `num_extra_cycles` is added to the transaction's current cycle count before the fee is
/// computed, so a caller can account for work that still lies ahead of it, such as signature
/// verification or the epilogue. `exclude_notes_commitment` commits to the output-note indices
/// that should be left out of the computation, or is the empty word when nothing is excluded.
///
/// # Panics
///
/// Panics if the computed fee exceeds the maximum asset amount.
pub fn compute_fee(num_extra_cycles: u32, exclude_notes_commitment: Word) -> AssetAmount {
    let fee = unsafe {
        extern_tx_compute_fee(
            Felt::from_u32(num_extra_cycles),
            exclude_notes_commitment[0],
            exclude_notes_commitment[1],
            exclude_notes_commitment[2],
            exclude_notes_commitment[3],
        )
    };
    AssetAmount::try_from(fee).expect("transaction fee exceeds the maximum asset amount")
}

/// Returns the asset id that transaction fees are paid in, as of the transaction reference block.
pub fn get_fee_asset_id() -> AssetId {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<AssetId>::uninit());
        extern_tx_get_fee_asset_id(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}
