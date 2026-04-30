/// Stubs for intrinsics::felt::* functions.
///
/// These are linked by name, and the frontend lowers calls to MASM operations or functions
/// accordingly.
define_stub! {
    #[unsafe(export_name = "intrinsics::felt::add")]
    pub extern "C" fn felt_add_stub(a: f32, b: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::from_u64_unchecked")]
    pub extern "C" fn felt_from_u64_unchecked_stub(v: u64) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::from_u32")]
    pub extern "C" fn felt_from_u32_stub(v: u32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::as_u64")]
    pub extern "C" fn felt_as_u64_stub(a: f32) -> u64;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::sub")]
    pub extern "C" fn felt_sub_stub(a: f32, b: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::mul")]
    pub extern "C" fn felt_mul_stub(a: f32, b: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::div")]
    pub extern "C" fn felt_div_stub(a: f32, b: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::neg")]
    pub extern "C" fn felt_neg_stub(a: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::inv")]
    pub extern "C" fn felt_inv_stub(a: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::pow2")]
    pub extern "C" fn felt_pow2_stub(a: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::exp")]
    pub extern "C" fn felt_exp_stub(a: f32, b: f32) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::eq")]
    pub extern "C" fn felt_eq_stub(a: f32, b: f32) -> i32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::gt")]
    pub extern "C" fn felt_gt_stub(a: f32, b: f32) -> i32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::lt")]
    pub extern "C" fn felt_lt_stub(a: f32, b: f32) -> i32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::ge")]
    pub extern "C" fn felt_ge_stub(a: f32, b: f32) -> i32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::le")]
    pub extern "C" fn felt_le_stub(a: f32, b: f32) -> i32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::is_odd")]
    pub extern "C" fn felt_is_odd_stub(a: f32) -> i32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::assert")]
    pub extern "C" fn felt_assert_stub(a: f32);
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::assertz")]
    pub extern "C" fn felt_assertz_stub(a: f32);
}

define_stub! {
    #[unsafe(export_name = "intrinsics::felt::assert_eq")]
    pub extern "C" fn felt_assert_eq_stub(a: f32, b: f32);
}
