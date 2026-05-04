// Test file to verify location expressions in debug info
// Using no_std to avoid runtime overhead

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn test_expressions(p0: i32, p1: i32, p2: i32, p3: i32) -> i32 {
    // These parameters should be in WASM locals 0, 1, 2, 3
    // The debug info expressions should show:
    // p0 -> DW_OP_WASM_local 0
    // p1 -> DW_OP_WASM_local 1
    // p2 -> DW_OP_WASM_local 2
    // p3 -> DW_OP_WASM_local 3

    // Simple arithmetic using all parameters
    let sum1 = p0.wrapping_add(p1);
    let sum2 = p2.wrapping_add(p3);
    sum1.wrapping_add(sum2)
}