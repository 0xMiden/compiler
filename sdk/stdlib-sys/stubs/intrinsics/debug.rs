/// Stubs for intrinsics::debug interface.
define_stub! {
    #[unsafe(export_name = "intrinsics::debug::break")]
    pub extern "C" fn debug_break_stub();
}
