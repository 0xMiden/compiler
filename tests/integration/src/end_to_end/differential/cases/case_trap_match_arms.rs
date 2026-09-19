// Trap parity: a dense `match` over a wrapped selector — the shape LLVM
// lowers to a `br_table` — where three of the eight arms are explicit
// panics. Arm 3 is `panic!`, arm 5 is `unreachable!()` (reached because the
// selector is `input1 % 8`, not a value the type system bounds) and arm 7 is
// `todo!()`; the other five must return. Both targets must trap on exactly
// the three trapping arms.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let sel = input1 % 8;
    let v = match sel {
        0 => input2.wrapping_add(1),
        1 => input2.rotate_left(3),
        2 => input2 ^ 0x5555_5555,
        3 => panic!("arm 3"),
        4 => input2.wrapping_mul(7),
        5 => unreachable!(),
        6 => input2.count_ones(),
        _ => todo!(),
    };
    v ^ sel
}
