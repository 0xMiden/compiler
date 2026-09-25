// A dense 8-arm `match` inside a 5-trip loop plus a 3-arm sparse match in a
// 4-trip loop: at -Oz (`--optimize=size-min`) both loops stay loops and
// each match is ONE `br_table` inside the loop body (O2 unrolls the trips
// into five/four separate dispatches), with arms that mutate loop state,
// `break`, or fall through — a br_table-in-kept-loop dispatch shape with
// a loop-carried selector.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut s = input1;
    let mut k = input2;
    let mut i = 0;
    while i < 5 {
        s = match (s ^ k) & 7 {
            0 => s.wrapping_add(k),
            1 => s.rotate_left(3),
            2 => s ^ 0xdead_beef,
            3 => s.wrapping_mul(5),
            4 => {
                k = k.rotate_right(1);
                s
            }
            5 => s.wrapping_sub(k),
            6 => s >> 1,
            _ => {
                if k == 0 {
                    break;
                }
                s | k
            }
        };
        k = k.wrapping_add(0x1234_5678);
        i += 1;
    }
    let mut j = 0;
    let mut t: u64 = ((s as u64) << 32) | (k as u64);
    while j < 4 {
        t = match (t >> (j * 5)) & 0x1f {
            3 => t.rotate_left(9),
            17 => t.wrapping_mul(0x9e37_79b9),
            29 => t ^ (input1 as u64),
            _ => t.wrapping_add(input2 as u64),
        };
        j += 1;
    }
    s ^ k ^ (t as u32) ^ ((t >> 32) as u32)
}
