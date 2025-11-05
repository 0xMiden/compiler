/// Unreachable stubs for intrinsics::advice interface

#[export_name = "intrinsics::advice::adv_push_mapvaln"]
pub extern "C" fn advice_adv_push_mapvaln_stub(
    _key0: f32,
    _key1: f32,
    _key2: f32,
    _key3: f32,
) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::advice::emit_falcon_sig_to_stack"]
pub extern "C" fn advice_emit_falcon_sig_to_stack_stub(
    _m0: f32,
    _m1: f32,
    _m2: f32,
    _m3: f32,
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::advice::adv_insert_mem"]
pub extern "C" fn advice_adv_insert_mem_stub(
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
    _start: i32,
    _end: i32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::advice::emit_and_verify_falcon"]
pub extern "C" fn advice_emit_and_verify_falcon_stub(
    _m0: f32,
    _m1: f32,
    _m2: f32,
    _m3: f32,
    _k0: f32,
    _k1: f32,
    _k2: f32,
    _k3: f32,
) {
    unsafe { core::hint::unreachable_unchecked() }
}
