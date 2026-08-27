// Function pointers as first-class VALUES: returned from and passed to
// `#[inline(never)]` helpers, mutated across loop iterations (state-machine
// style), coerced from a non-capturing closure, and compared with `==`. At the
// Wasm level a fn pointer is its funcref-table index (an i32), so all of this
// exercises table-index data flow between functions, across loop-carried
// locals, and through an integer comparison — while every actual dispatch
// stays a runtime-indexed `call_indirect`.

type Op = fn(u32, u32) -> u32;

#[inline(never)]
fn op_add(a: u32, b: u32) -> u32 {
    a.wrapping_add(b)
}

#[inline(never)]
fn op_shear(a: u32, b: u32) -> u32 {
    (a ^ b).rotate_left(11)
}

#[inline(never)]
fn op_scale(a: u32, b: u32) -> u32 {
    a.wrapping_mul(b | 1)
}

// Returns a fn pointer picked by runtime data: the caller receives a table
// index it cannot devirtualize through the noinline boundary.
#[inline(never)]
fn pick(sel: u32) -> Op {
    match sel % 3 {
        0 => op_add,
        1 => op_shear,
        _ => op_scale,
    }
}

// Takes a fn pointer as a parameter and dispatches through it.
#[inline(never)]
fn apply(f: Op, a: u32, b: u32) -> u32 {
    f(a, b)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut f = pick(input1);
    let g = pick(input2 >> 3);
    // fn-pointer equality compares funcref-table indices in wasm and host
    // addresses natively; both are injective over these distinct functions.
    let same = (f == g) as u32;
    // Non-capturing closure coerced to `fn`: an anonymous table entry.
    let h: Op = |a, b| (a | 3).wrapping_sub(b >> 2);
    let mut acc = apply(h, input2, input1);
    // Loop-carried fn-pointer state machine: f changes each iteration based
    // on data computed through the previous pointer.
    let mut i = 0u32;
    while i < 4 {
        acc = apply(f, acc, input1.rotate_left(i));
        f = pick(acc ^ i);
        i += 1;
    }
    acc.wrapping_add(g(acc, input2)).wrapping_add(same)
}
