// Trap parity behind an indirect call: the panicking operation is selected
// through a function-pointer table read with `core::hint::black_box`, which
// keeps the dispatch a `call_indirect` instead of the switch of direct calls
// the nightly-2026-09-01 guest toolchain devirtualizes it into. Slot 2 traps
// on a failed `assert!`, slot 3 traps on a bounds check, slots 0 and 1
// always return; both targets must trap on exactly the trapping slot/argument
// combinations.
fn f_ok(x: u32) -> u32 {
    x.wrapping_add(0x1111_1111)
}

fn f_rot(x: u32) -> u32 {
    x.rotate_left(5)
}

fn f_assert(x: u32) -> u32 {
    assert!(x % 3 != 1, "trapping slot");
    x ^ 0x2222_2222
}

fn f_index(x: u32) -> u32 {
    let t: [u32; 4] = [9, 8, 7, 6];
    t[(x % 6) as usize]
}

static OPS: [fn(u32) -> u32; 4] = [f_ok, f_rot, f_assert, f_index];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let table = core::hint::black_box(&OPS);
    let f = table[(input1 % 4) as usize];
    f(input2) ^ input1
}
