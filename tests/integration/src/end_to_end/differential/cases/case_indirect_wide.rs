// The widest indirect signature the lowering accepts: 7 u64 parameters are 14
// stack felts, plus the table index = 15 of Miden's 16-element operand-stack
// window (16 argument felts + the index would be diagnosed at translation).
// Dispatching it exercises `dynexec` with a full argument window and u64
// (two-felt) values crossing the dispatch boundary in both directions.

type Wide = fn(u64, u64, u64, u64, u64, u64, u64) -> u64;

#[inline(never)]
fn w_fold(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64) -> u64 {
    a.wrapping_add(b)
        .wrapping_mul(c | 1)
        .wrapping_sub(d)
        .rotate_left((e & 63) as u32)
        ^ f.wrapping_add(g)
}

#[inline(never)]
fn w_zip(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64) -> u64 {
    (a ^ b.rotate_right(17))
        .wrapping_add(c.wrapping_mul(c))
        .wrapping_add(d >> 3)
        .wrapping_add(e << 5)
        .wrapping_add(f ^ g.swap_bytes())
}

static WIDES: [Wide; 2] = [w_fold, w_zip];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = ((input1 as u64) << 32) | input2 as u64;
    let y = ((input2 as u64) << 32) | input1 as u64;
    let f = WIDES[(input1 & 1) as usize];
    let r = f(
        x,
        y,
        x.wrapping_add(y),
        x ^ 0x00ff_00ff_00ff_00ff,
        y.wrapping_mul(3),
        x.rotate_left(9),
        y ^ x,
    );
    (r as u32).wrapping_add((r >> 32) as u32)
}
