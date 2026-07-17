// A br_table inside a loop whose arms exit in five different ways
// (fallthrough, break, continue, return, impossible trap). Mixes in-loop and
// out-of-loop switch successors, producing nested index_switches (a
// discriminator switch inside a user-switch arm), three-result switches, and
// an ub.unreachable-terminated tail through cfg-to-scf structuring.

/// Trap lowering to a genuine wasm `unreachable`; never executed (impossible
/// guards at call sites).
#[inline(always)]
fn trap() -> ! {
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_arch = "wasm32"))]
    loop {}
}

#[inline(never)]
fn switch_loop_mix(a: u32, b: u32) -> u32 {
    let mut x = a | 1;
    let mut acc = 0u32;
    let mut i = 0u32;
    loop {
        x = x.wrapping_mul(0x0100_0193) ^ i;
        match x % 8 {
            0 => acc = acc.wrapping_add(x >> 3),
            1 => {
                if x & 0xfff == 0x600 {
                    break; // arm exits the loop
                }
                acc ^= x;
            }
            2 => {
                let h = x ^ b;
                if h % 6 == 5 && h % 3 == 0 {
                    trap(); // impossible trap arm
                }
                acc = acc.rotate_left(1);
            }
            3 => {
                if i > (b % 61) {
                    return acc ^ x; // arm returns from the function
                }
            }
            4 => acc ^= x.rotate_right(7),
            5 => {
                i = i.wrapping_add(2);
                continue; // arm re-enters the loop header
            }
            6 => acc = acc.wrapping_sub(x),
            _ => acc = acc.wrapping_add(x | 5),
        }
        i = i.wrapping_add(1);
        if i >= (b % 97).wrapping_add(3) {
            break;
        }
    }
    acc ^ x ^ i.wrapping_mul(0x9e37_79b9)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    switch_loop_mix(input1, input2)
}
