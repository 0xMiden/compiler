// Many simultaneously-live NAMED locals with staggered ranges across a
// branch and an opaque call: a dense location schedule (declares and kills
// interleaved through the function), and in MASM one DebugVar decorator plus
// a REAL Nop per record woven through the operand scheduling of live
// arithmetic. Debug info must never change semantics — the Nops and
// decorators must be neutral under pressure.

use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn dbg_ml_opaque(x: u32) -> u32 {
    // Opaque side effect pins the call in place (never changes state).
    x.wrapping_add(PIN.fetch_add(0, Ordering::Relaxed))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let alpha = input1 ^ 0x0a0b_0c0d;
    let bravo = input2.wrapping_mul(0x9e37_79b1);
    let charlie = alpha.rotate_left(3) ^ bravo;
    let delta = bravo.wrapping_sub(alpha).rotate_right(5);
    let echo = charlie.wrapping_add(delta) | 1;
    let foxtrot = (alpha & bravo).wrapping_mul(3);
    let golf = charlie ^ delta.wrapping_add(0x5851_f42d);
    let hotel = dbg_ml_opaque(echo ^ foxtrot);
    let india = if hotel & 1 == 0 {
        let juliet = golf.wrapping_add(hotel);
        juliet.rotate_left(7) ^ alpha
    } else {
        let kilo = golf.wrapping_sub(hotel);
        kilo.rotate_right(9) ^ bravo
    };
    india
        .wrapping_add(charlie)
        .wrapping_add(delta)
        .wrapping_add(echo)
        .wrapping_add(foxtrot)
        .wrapping_add(golf)
}
