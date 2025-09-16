use crate::intrinsics::{Felt, Word};

extern "C" {
    #[link_name = "std::crypto::dsa::rpo_falcon512::verify"]
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
/// The procedure relies on the `adv.push_sig` decorator to retrieve the signature from the host.
/// The default host implementation assumes that the private-public key pair is loaded into the
/// advice provider, and uses it to generate the signature. However, for production grade
/// implementations, this functionality should be overridden to ensure more secure handling of
/// private keys.
#[inline(always)]
pub fn rpo_falcon512_verify(pk: Word, msg: Word) {
    unsafe {
        extern_rpo_falcon512_verify(pk[0], pk[1], pk[2], pk[3], msg[0], msg[1], msg[2], msg[3]);
    }
}
