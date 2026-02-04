#![no_std]

/// Unreachable stub for `std::crypto::dsa::rpo_falcon512::verify`.
///
/// This satisfies link-time references and allows the compiler to lower calls to MASM.
#[unsafe(export_name = "miden::core::crypto::dsa::rpo_falcon512::verify")]
pub extern "C" fn rpo_falcon512_verify_stub(
    _pk1: f32,
    _pk2: f32,
    _pk3: f32,
    _pk4: f32,
    _msg1: f32,
    _msg2: f32,
    _msg3: f32,
    _msg4: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
