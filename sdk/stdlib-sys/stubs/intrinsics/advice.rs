/// Unreachable stubs for intrinsics::advice interface

#[export_name = "intrinsics::advice::adv_push_mapvaln"]
pub extern "C" fn advice_adv_push_mapvaln_stub(_key0: f32, _key1: f32, _key2: f32, _key3: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

