use crate::{intrinsics::Word, raw::core::crypto::dsa::falcon512_poseidon2};

/// Verifies a signature against a public key and a message. The procedure gets as inputs the hash
/// of the public key and the hash of the message via the operand stack. The signature is expected
/// to be provided via the advice provider. The signature is valid if and only if the procedure
/// returns.
///
/// Where `pk` is the hash of the public key and `msg` is the hash of the message. Both hashes are
/// expected to be computed using Poseidon2.
///
/// The verification expects the signature to be provided by the host via the advice stack.
/// In the current flow, callers should first trigger a signature request event using
/// `crate::emit_falcon_sig_to_stack(msg, pk)` and then call this function. The host must respond by
/// pushing the signature to the advice stack. For production deployments, ensure secret key
/// handling occurs outside the VM.
#[inline(always)]
pub fn rpo_falcon512_verify(pk: Word, msg: Word) {
    falcon512_poseidon2::verify(pk, msg)
}
