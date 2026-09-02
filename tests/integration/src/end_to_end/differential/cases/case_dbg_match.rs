// Match-arm-scoped NAMED locals over a dense `br_table` dispatch: every arm
// binds its own variables (scoped DWARF ranges beginning and ending at arm
// boundaries), driving arm-local declare/kill schedule events through the
// switch lowering. Debug info must never change semantics — arm selection
// and results depend only on the inputs.

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let lane = input1 & 7;
    let fuel = input2 | 1;
    match lane {
        0 => {
            let zero_mix = fuel.wrapping_mul(3);
            zero_mix ^ input1
        }
        1 => {
            let one_rot = fuel.rotate_left(9);
            one_rot.wrapping_add(input1)
        }
        2 => {
            let two_sub = fuel.wrapping_sub(input1);
            two_sub.rotate_right(3)
        }
        3 => {
            let three_and = fuel & input1;
            three_and.wrapping_mul(0x0101_0101)
        }
        4 => {
            let four_or = fuel | input1;
            four_or.rotate_left(fuel & 15)
        }
        5 => {
            let five_xor = fuel ^ 0x5a5a_5a5a;
            five_xor.wrapping_add(input1.rotate_right(7))
        }
        6 => {
            let six_shift = fuel >> (input1 & 15);
            six_shift.wrapping_mul(fuel | 3)
        }
        _ => {
            let seven_mix = fuel.wrapping_mul(fuel | 7);
            seven_mix ^ input1.rotate_left(11)
        }
    }
}
