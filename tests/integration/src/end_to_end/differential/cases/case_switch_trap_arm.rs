// A br_table dispatch where one arm ends in a dynamically-impossible panic
// (cross-modulus contradiction). The switch then has successor regions with
// mixed return-like terminators (yield/ret vs unreachable), exercising
// cfg-to-scf structuring of branch regions that cannot share an exit block
// and index_switch regions ending in traps. (The no-exit-edges
// `create_unreachable_terminator` path is covered by `case_unreachable_exits`.)
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let h = input1 ^ input2.rotate_left(11);
    let sel = h % 5;
    match sel {
        0 => h.wrapping_mul(3) ^ input2,
        1 => h.rotate_left(7).wrapping_add(input1),
        2 => {
            // Impossible: h % 10 == 7 implies h % 5 == 2 is fine, but
            // h % 10 == 4 implies h % 5 == 4, contradicting sel == 2.
            if h % 10 == 4 {
                panic!();
            }
            h >> 2
        }
        3 => h.wrapping_sub(input1).rotate_right(3),
        _ => h ^ 0x5555_aaaa,
    }
}
