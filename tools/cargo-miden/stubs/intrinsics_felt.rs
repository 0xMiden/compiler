use core::ffi::c_void;

/// Unreachable stubs for intrinsics::felt::* functions.
/// These are linked by name, and the frontend lowers calls
/// to MASM operations or functions accordingly.

#[export_name = "intrinsics::felt::add"]
pub extern "C" fn felt_add_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::from_u64_unchecked"]
pub extern "C" fn felt_from_u64_unchecked_stub(_v: u64) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::from_u32"]
pub extern "C" fn felt_from_u32_stub(_v: u32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::as_u64"]
pub extern "C" fn felt_as_u64_stub(_a: f32) -> u64 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::sub"]
pub extern "C" fn felt_sub_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::mul"]
pub fn felt_mul_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::div"]
pub extern "C" fn felt_div_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::neg"]
pub extern "C" fn felt_neg_stub(_a: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::inv"]
pub extern "C" fn felt_inv_stub(_a: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::pow2"]
pub extern "C" fn felt_pow2_stub(_a: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::exp"]
pub extern "C" fn felt_exp_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::eq"]
pub extern "C" fn felt_eq_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::gt"]
pub extern "C" fn felt_gt_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::lt"]
pub extern "C" fn felt_lt_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::ge"]
pub extern "C" fn felt_ge_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::le"]
pub extern "C" fn felt_le_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::is_odd"]
pub extern "C" fn felt_is_odd_stub(_a: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::assert"]
pub extern "C" fn felt_assert_stub(_a: f32) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::assertz"]
pub extern "C" fn felt_assertz_stub(_a: f32) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[export_name = "intrinsics::felt::assert_eq"]
pub extern "C" fn felt_assert_eq_stub(_a: f32, _b: f32) {
    unsafe { core::hint::unreachable_unchecked() }
}

