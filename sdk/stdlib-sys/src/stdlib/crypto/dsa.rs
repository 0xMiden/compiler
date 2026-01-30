#[cfg(not(all(target_family = "wasm", miden)))]
use crate::intrinsics::Word;
#[cfg(all(target_family = "wasm", miden))]
use crate::intrinsics::{Felt, Word};

#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
    #[link_name = "miden::core::crypto::dsa::rpo_falcon512::verify"]
    fn extern_rpo_falcon512_verify(
        pk1: Felt,
        pk2: Felt,
        pk3: Felt,
        pk4: Felt,
        msg1: Felt,
        msg2: Felt,
        msg3: Felt,
        msg4: Felt,
    );
}

/// Verifies a signature against a public key and a message. The procedure gets as inputs the hash
/// of the public key and the hash of the message via the operand stack. The signature is expected
/// to be provided via the advice provider. The signature is valid if and only if the procedure
/// returns.
///
/// Where `pk` is the hash of the public key and `msg` is the hash of the message. Both hashes are
/// expected to be computed using RPO hash function.
///
/// The verification expects the signature to be provided by the host via the advice stack.
/// In the current flow, callers should first trigger a signature request event using
/// `crate::emit_falcon_sig_to_stack(msg, pk)` and then call this function. The host must respond by
/// pushing the signature to the advice stack. For production deployments, ensure secret key
/// handling occurs outside the VM.
#[inline(always)]
#[cfg(all(target_family = "wasm", miden))]
pub fn rpo_falcon512_verify(pk: Word, msg: Word) {
    unsafe {
        extern_rpo_falcon512_verify(pk[3], pk[2], pk[1], pk[0], msg[3], msg[2], msg[1], msg[0]);
    }
}

#[inline(always)]
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn rpo_falcon512_verify(_pk: Word, _msg: Word) {
    unimplemented!("miden::core::crypto::dsa bindings are only available when targeting the Miden VM")
}
