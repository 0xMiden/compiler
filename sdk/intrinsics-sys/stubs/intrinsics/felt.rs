/// Unreachable stubs for intrinsics::felt::* functions.
/// These are linked by name, and the frontend lowers calls
/// to MASM operations or functions accordingly.

#[unsafe(export_name = "intrinsics::felt::add")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_add_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::from_u64_unchecked")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_from_u64_unchecked_stub(_v: u64) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::from_u32")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_from_u32_stub(_v: u32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::as_u64")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_as_u64_stub(_a: f32) -> u64 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::sub")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_sub_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::mul")]
#[optimize(none)]
#[inline(never)]
pub fn felt_mul_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::div")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_div_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::neg")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_neg_stub(_a: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::inv")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_inv_stub(_a: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::pow2")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_pow2_stub(_a: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::exp")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_exp_stub(_a: f32, _b: f32) -> f32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::eq")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_eq_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::gt")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_gt_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::lt")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_lt_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::ge")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_ge_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::le")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_le_stub(_a: f32, _b: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::is_odd")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_is_odd_stub(_a: f32) -> i32 {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::assert")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_assert_stub(_a: f32) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::assertz")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_assertz_stub(_a: f32) {
    unsafe { core::hint::unreachable_unchecked() }
}

#[unsafe(export_name = "intrinsics::felt::assert_eq")]
#[optimize(none)]
#[inline(never)]
pub extern "C" fn felt_assert_eq_stub(_a: f32, _b: f32) {
    unsafe { core::hint::unreachable_unchecked() }
}
