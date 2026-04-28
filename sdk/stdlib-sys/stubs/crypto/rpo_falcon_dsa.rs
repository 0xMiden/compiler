/// Stub for `std::crypto::dsa::rpo_falcon512::verify`.
///
/// This satisfies link-time references and allows the compiler to lower calls to MASM.
define_stub! {
    #[unsafe(export_name = "miden::core::crypto::dsa::falcon512_poseidon2::verify")]
    pub extern "C" fn rpo_falcon512_verify_stub(
        pk1: f32,
        pk2: f32,
        pk3: f32,
        pk4: f32,
        msg1: f32,
        msg2: f32,
        msg3: f32,
        msg4: f32,
    );
}
