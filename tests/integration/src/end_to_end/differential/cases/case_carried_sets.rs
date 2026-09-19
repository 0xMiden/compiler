// Loop-carried variable sets whose update ORDER matters: a five-variable
// loop with a three-way rotation `(a, b, c) = (b, c, a)`, a swap, arms that
// update only a subset of the variables, and a variable `e` that is READ
// only on the back-edge path; plus a nine-variable loop with a full
// rotation of all nine and a per-arm partial update, driven by a selector
// that is itself loop-carried. Both loops use `% m + 1` bounds (at least
// one trip, no peeling).

// A: five carried variables, rotation / swap / subset updates.
#[inline(never)]
fn rot5(input1: u32, input2: u32) -> u32 {
    let mut a = input1;
    let mut b = input2;
    let mut c = input1 ^ input2;
    let mut d = input1.rotate_left(7);
    let mut e = 0x1234_5678u32;
    let mut sel = input1;
    let n = (input2 % 61).wrapping_add(1);
    let mut i = 0u32;
    while i < n {
        match sel & 3 {
            0 => {
                let t = a;
                a = b;
                b = c;
                c = t;
            }
            1 => {
                core::mem::swap(&mut a, &mut d);
                e = e.wrapping_add(c);
            }
            2 => d = d.wrapping_mul(3) ^ b,
            _ => b = b.rotate_left(5).wrapping_add(e),
        }
        i = i.wrapping_add(1);
        if i >= n {
            break; // `e` and `sel` are not touched on the exit path
        }
        sel = if a & 1 == 1 {
            sel.rotate_right(2) ^ e
        } else {
            sel >> 2
        };
    }
    a ^ b.rotate_left(3) ^ c.rotate_left(6) ^ d.rotate_left(9) ^ e ^ sel
}

// B: nine carried variables, full rotation each trip plus one partial update.
#[inline(never)]
fn rot9(input1: u32, input2: u32) -> u32 {
    let mut v = [
        input1,
        input2,
        input1 ^ 0x1111,
        input2 ^ 0x2222,
        input1.rotate_left(3),
        input2.rotate_left(5),
        input1.wrapping_add(input2),
        input1.wrapping_sub(input2),
        0x0f0f_0f0fu32,
    ];
    let (mut v0, mut v1, mut v2, mut v3, mut v4, mut v5, mut v6, mut v7, mut v8) =
        (v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8]);
    let mut sel = input2;
    let n = (input1 % 23).wrapping_add(1);
    let mut i = 0u32;
    while i < n {
        // Full rotation: every variable takes its right neighbour's value.
        let t = v0;
        v0 = v1;
        v1 = v2;
        v2 = v3;
        v3 = v4;
        v4 = v5;
        v5 = v6;
        v6 = v7;
        v7 = v8;
        v8 = t;
        match sel % 5 {
            0 => v0 = v0.wrapping_add(v4),
            1 => v3 ^= v7,
            2 => {
                v5 = v5.rotate_left(1);
                v8 = v8.wrapping_sub(v1);
            }
            3 => core::mem::swap(&mut v2, &mut v6),
            _ => {}
        }
        sel = sel.wrapping_mul(0x9e37_79b9) ^ v0;
        i = i.wrapping_add(1);
    }
    v = [v0, v1, v2, v3, v4, v5, v6, v7, v8];
    let mut acc = sel;
    let mut k = 0usize;
    while k < 9 {
        acc = acc.rotate_left(3) ^ v[k];
        k += 1;
    }
    acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    rot5(input1, input2) ^ rot9(input2, input1).rotate_left(11)
}
