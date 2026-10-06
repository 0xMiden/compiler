// Arm merging with a per-arm pinned grid: a sixteen-arm `match` on an
// input-derived selector where five SCATTERED arms have a body identical to
// the default arm's. LLVM points those `br_table` entries at the fallback
// block, so `SimplifySwitchFallbackOverlap` rebuilds the `cf.switch` without
// them -- one rewrite removing several cases at once. The selector is returned
// in the top nibble, so a grid can pin one input per arm (merged arms, kept
// arms and the true default alike): a wrong case key or a dropped non-
// overlapping case is a silent miscompile, not a panic.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let h = input1 ^ input2.rotate_left(11);
    let sel = h & 15;
    let v = match sel {
        0 => h.wrapping_add(7),
        1 => h.rotate_left(5),
        2 => h.wrapping_mul(3),
        3 => h.wrapping_add(7),
        4 => h.wrapping_sub(11),
        5 => h ^ 0x5a5a_5a5a,
        6 => h.wrapping_add(7),
        7 => h.rotate_right(9),
        8 => h.wrapping_mul(5),
        9 => h.wrapping_add(7),
        10 => h ^ input2,
        11 => h.wrapping_sub(input1),
        12 => h.wrapping_add(7),
        13 => h.wrapping_add(input1.wrapping_mul(3)),
        14 => h.rotate_left(13),
        _ => h.wrapping_add(7),
    };
    (sel << 28) | (v & 0x0fff_ffff)
}
