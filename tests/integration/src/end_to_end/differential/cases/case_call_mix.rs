// Call arity/position edges: a zero-arg zero-result helper (empty
// required-operands early return in operand scheduling), a zero-arg helper
// with a result, and helper calls sitting inside a loop body and inside both
// branches of a conditional (exec ops in non-entry regions). State flows
// through an atomic static (restored before returning, for native-side
// determinism across the 16 reused invocations).

use core::sync::atomic::{AtomicU32, Ordering};

static ACC: AtomicU32 = AtomicU32::new(0x1234_5678);

/// Zero-arg, zero-result call.
#[inline(never)]
fn tick() {
    let v = ACC.load(Ordering::Relaxed);
    ACC.store(v.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15), Ordering::Relaxed);
}

/// Zero-arg call with a result.
#[inline(never)]
fn sample() -> u32 {
    ACC.load(Ordering::Relaxed)
}

#[inline(never)]
fn stir(x: u32, y: u32) -> u32 {
    x.rotate_left(y & 31) ^ y.wrapping_mul(0x85eb_ca6b)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    tick();
    let a = sample();
    let mut acc = input1;
    let mut i = 0u32;
    let n = input2 % 13;
    while i < n {
        // Calls inside the loop body, including a zero-arg zero-result one.
        acc = stir(acc, i ^ input2);
        tick();
        i = i.wrapping_add(1);
    }
    let b = sample();
    // Calls inside both branches of a conditional.
    let r = if acc & 1 == 0 {
        stir(acc, b)
    } else {
        stir(b, acc ^ a)
    };
    // Restore the static so the reused native cdylib stays deterministic.
    ACC.store(0x1234_5678, Ordering::Relaxed);
    r ^ a ^ b
}
