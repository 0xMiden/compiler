// Control flow dispatched on `Ordering` values: a state machine whose
// transitions match on the pair (unsigned compare, signed compare) of two
// evolving values — the two orderings differ exactly when the sign bits
// differ — a direct three-way `match a.cmp(&b)`, and `min`/`max`/`clamp`
// selects mixed into the same loop. The i8 `Ordering` selectors are
// produced by compare-and-select chains and consumed by br_tables.
use core::cmp::Ordering;

#[inline(never)]
fn cmp_sm(a: u32, b: u32) -> u32 {
    let mut x = a;
    let mut y = b;
    let mut acc = 0u32;
    let mut state = 0u32;
    let n = (b % 37).wrapping_add(1);
    let mut i = 0u32;
    while i < n {
        let ord_u = x.cmp(&y);
        let ord_s = (x as i32).cmp(&(y as i32));
        state = match (state, ord_u, ord_s) {
            (0, Ordering::Less, _) => 1,
            (0, Ordering::Equal, Ordering::Equal) => 2,
            (0, ..) => 3,
            (1, _, Ordering::Less) => {
                acc = acc.wrapping_add(x);
                2
            }
            (1, _, Ordering::Greater) => {
                acc ^= y;
                3
            }
            (1, ..) => 0,
            (2, Ordering::Greater, Ordering::Less) => {
                acc = acc.rotate_left(1);
                0
            }
            (2, Ordering::Less, Ordering::Greater) => {
                acc = acc.rotate_right(1);
                3
            }
            (2, ..) => 1,
            (_, o1, o2) if o1 == o2 => {
                acc = acc.wrapping_mul(3);
                0
            }
            _ => {
                acc = acc.wrapping_sub(1);
                2
            }
        };
        x = x.wrapping_mul(0x9e37_79b9) ^ i;
        y = y.rotate_left(5).wrapping_add(state);
        i = i.wrapping_add(1);
    }
    acc ^ state.wrapping_mul(0x0101_0101) ^ x ^ y
}

#[inline(never)]
fn three_way(a: u32, b: u32) -> u32 {
    let k = match a.cmp(&b) {
        Ordering::Less => a.wrapping_sub(b),
        Ordering::Equal => 0x5555_5555,
        Ordering::Greater => b.wrapping_sub(a).rotate_left(1),
    };
    let s = match (a as i32).cmp(&(b as i32)).then((b & 7).cmp(&(a & 7))) {
        Ordering::Less => 1u32,
        Ordering::Equal => 2,
        Ordering::Greater => 3,
    };
    let m = a.max(b).wrapping_sub(a.min(b)) ^ (a as i32).clamp(-1000, 1000) as u32;
    k ^ s.wrapping_mul(0x0100_0100) ^ m
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    cmp_sm(input1, input2) ^ three_way(input2, input1).rotate_left(13)
}
