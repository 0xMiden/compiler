#![no_std]
#![no_main]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

enum Op {
    Add,
    Sub,
    Mul,
}

#[inline(never)]
#[unsafe(no_mangle)]
fn match_enum(a: u32, b: u32, foo: Op) -> u32 {
    match foo {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul => a * b,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __main() -> u32 {
    match_enum(3, 5, Op::Add) + match_enum(3, 5, Op::Sub) + match_enum(3, 5, Op::Mul)
}
