// Trap parity: nested loops whose inner trip count comes from a `static`
// table, so the index that eventually runs off the end of `buf` depends on a
// data-segment lookup as well as on both inputs. The inner bound is
// `LIMITS[input2 % 5]` and trip `(o, i)` reads `buf[input1 % 7 + 2*o + i]`,
// so the largest index is `input1 % 7 + lim + 1`: with `lim == 2` the trap
// first appears at `input1 % 7 == 5`, on the very last trip of the last
// outer iteration, after every earlier trip has already folded a value in.
static LIMITS: [u32; 5] = [4, 3, 2, 3, 4];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let buf: [u32; 8] = [1, 3, 5, 7, 9, 11, 13, 15];
    let base = input1 % 7;
    let lim = LIMITS[(input2 % 5) as usize];
    let mut acc = input1 ^ 0x1234_5678;
    let mut o = 0u32;
    while o < 2 {
        let mut i = 0u32;
        while i < lim {
            let k = (base + o * 2 + i) as usize;
            acc = acc.rotate_left(3) ^ buf[k];
            i += 1;
        }
        o += 1;
    }
    acc
}
