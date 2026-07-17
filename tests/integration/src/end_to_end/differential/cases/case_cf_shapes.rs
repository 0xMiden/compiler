// Four CF shapes the corpus lacked (verified structurally new via HIR dump):
// labeled break/continue through two loop levels (nested scf.while with a
// three-result inner while + chained discriminator index_switches), a loop
// whose exits all leave state in locals (zero-result index_switch with empty
// arms), a loop-produced bool feeding a post-loop branch, and a dense match
// with distinct-constant early returns (chained user + discriminator
// switches).

// A: labeled break through two loop levels.
#[inline(never)]
fn labeled_break(a: u32, b: u32) -> u32 {
    let mut x = a | 1;
    let mut acc = 0u32;
    'outer: for i in 0..(b % 31).wrapping_add(2) {
        let mut j = 0u32;
        while j < (a % 17).wrapping_add(2) {
            x = x.wrapping_mul(0x0100_0193) ^ j;
            if x & 0xfff == 0x421 {
                acc = x ^ i;
                break 'outer;
            }
            if x & 0xff == 0x33 {
                // continue outer loop from inner body
                continue 'outer;
            }
            j = j.wrapping_add(1);
        }
        acc = acc.wrapping_add(x >> 3);
    }
    acc ^ x
}

// B: two exits exporting the same variable.
#[inline(never)]
fn two_exits_same_val(a: u32, b: u32) -> u32 {
    let mut x = a | 1;
    let mut i = 0u32;
    loop {
        x = x.wrapping_mul(0x0101_0101) ^ i;
        if x & 0x8000_000f == 0x8000_0003 {
            break; // exit 1: x live-out
        }
        if x & 0x8000_00f0 == 0x8000_0050 {
            break; // exit 2: x live-out (same value)
        }
        i = i.wrapping_add(1);
        if i >= (b % 89).wrapping_add(2) {
            break; // exit 3
        }
    }
    x
}

// C: loop exporting a bool decided at different exits.
#[inline(never)]
fn bool_escape(a: u32, b: u32) -> u32 {
    let mut x = a | 1;
    let mut i = 0u32;
    let found = loop {
        x = x.wrapping_mul(0x9e37_79b9) ^ i;
        if x & 0xffff == 0x1234 {
            break true;
        }
        i = i.wrapping_add(1);
        if i >= (b % 97).wrapping_add(1) {
            break false;
        }
    };
    if found {
        x ^ 0xdead_beef
    } else {
        x.wrapping_add(i)
    }
}

// D: match with distinct-constant early returns (no tail-merge possible).
#[inline(never)]
fn early_ret_consts(a: u32, b: u32) -> u32 {
    let h = a ^ b.rotate_left(9);
    match h % 7 {
        0 => 11,
        1 => 22,
        2 => 33,
        3 => h ^ b,
        4 => 44,
        5 => h.wrapping_add(a),
        _ => 55,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    labeled_break(input1, input2)
        ^ two_exits_same_val(input2, input1)
        ^ bool_escape(input1, input2)
        ^ early_ret_consts(input2, input1)
}
