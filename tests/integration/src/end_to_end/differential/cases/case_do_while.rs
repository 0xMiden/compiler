// Bottom-tested loops: a do-while whose exit condition is computed in the
// body (and read after the loop) with a `continue` that skips the bottom
// test and a mid-body break; a `while go` loop whose condition is a
// loop-carried bool updated in several arms; and a do-while nested inside
// a do-while where the inner condition also decides the outer one.
// Exit tag = top nibble (which site left the outer loop).

#[inline(never)]
fn do_while_cont(a: u32, b: u32) -> u32 {
    let mut x = a | 1;
    let mut i = 0u32;
    let mut cond;
    let n = (b % 53).wrapping_add(1);
    let tag = loop {
        x = x.wrapping_mul(0x0808_8405) ^ i;
        i = i.wrapping_add(1);
        cond = x & 0xf0 != 0x30 && i < n;
        if x & 7 == 1 {
            x = x.rotate_left(3);
            if cond {
                continue; // skips the bottom test
            }
            break 2u32;
        }
        if x & 0xf00 == 0x500 {
            break 3; // mid-body exit
        }
        x ^= i << 4;
        if !cond {
            break 1; // bottom test
        }
    };
    (tag << 28) | ((x ^ i ^ ((cond as u32) << 27)) & 0x0fff_ffff)
}

#[inline(never)]
fn carried_bool(a: u32, b: u32) -> u32 {
    let mut go = true;
    let mut x = a;
    let mut i = 0u32;
    let mut seen = 0u32;
    while go {
        x = x.wrapping_mul(0x9e37_79b9).wrapping_add(b);
        i = i.wrapping_add(1);
        match x >> 29 {
            0 => go = i < 40,
            1 => {
                seen ^= x;
                go = x & 0x100 == 0 && i < 50;
            }
            2 => seen = seen.wrapping_add(i),
            3 => {
                if i > 6 {
                    go = false;
                }
            }
            _ => go = i < 30 || seen & 1 == 1,
        }
    }
    x ^ i.wrapping_mul(0x0101_0101) ^ seen
}

#[inline(never)]
fn nested_do_while(a: u32, b: u32) -> u32 {
    let mut x = a ^ 0x1234;
    let mut outer = 0u32;
    let mut total = 0u32;
    loop {
        let mut inner = 0u32;
        let inner_ok = loop {
            x = x.wrapping_mul(0x0100_0193) ^ inner;
            inner = inner.wrapping_add(1);
            total = total.wrapping_add(1);
            if x & 0x3f == 0x15 {
                break false;
            }
            if inner >= (x % 5).wrapping_add(1) {
                break true;
            }
        };
        outer = outer.wrapping_add(1);
        if !inner_ok || outer >= (b % 23).wrapping_add(1) {
            break;
        }
    }
    x ^ outer.rotate_left(8) ^ total.rotate_left(16)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let t = do_while_cont(input1, input2);
    let low = carried_bool(input2, input1) ^ nested_do_while(input1, input2).rotate_left(11);
    (t & 0xf000_0000) | ((t ^ low) & 0x0fff_ffff)
}
