/// Stubs for intrinsics::advice interface.
define_stub! {
    #[unsafe(export_name = "intrinsics::advice::adv_push_mapvaln")]
    pub extern "C" fn advice_adv_push_mapvaln_stub(
        key0: f32,
        key1: f32,
        key2: f32,
        key3: f32,
    ) -> f32;
}

define_stub! {
    #[unsafe(export_name = "intrinsics::advice::emit_falcon_sig_to_stack")]
    pub extern "C" fn advice_emit_falcon_sig_to_stack_stub(
        m0: f32,
        m1: f32,
        m2: f32,
        m3: f32,
        k0: f32,
        k1: f32,
        k2: f32,
        k3: f32,
    );
}

define_stub! {
    #[unsafe(export_name = "intrinsics::advice::adv_insert_mem")]
    pub extern "C" fn advice_adv_insert_mem_stub(
        k0: f32,
        k1: f32,
        k2: f32,
        k3: f32,
        start: i32,
        end: i32,
    );
}
