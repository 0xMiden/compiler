// Aggregate (sret) call boundaries: tuple returns (u32,u32)/(u64,u32), a
// 5-field struct return, and a [u32; 8] array return all lower to
// sret-pointer calls (no +multivalue), i.e. zero-result `hir.exec` ops with
// void `builtin.ret` callees; big by-value struct/array params arrive as a
// pointer into the caller's stack frame. Exercises the sret/memory call
// path end to end: caller frame setup, pointer args, callee stores, caller
// reloads.

#[derive(Clone, Copy)]
pub struct Five {
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
}

#[inline(never)]
fn pair_ret(x: u32, y: u32) -> (u32, u32) {
    (x ^ y.rotate_left(3), x.wrapping_add(y))
}

#[inline(never)]
fn mixed_ret(x: u64, y: u32) -> (u64, u32) {
    (x.wrapping_mul(y as u64 | 1), y.rotate_left(5) ^ (x as u32))
}

#[inline(never)]
fn five_ret(x: u32) -> Five {
    Five {
        a: x,
        b: x ^ 0x5555_5555,
        c: x.wrapping_mul(3),
        d: x >> 3,
        e: x | 7,
    }
}

#[inline(never)]
fn arr_ret(x: u32) -> [u32; 8] {
    [
        x,
        x ^ 1,
        x.wrapping_add(2),
        x ^ 3,
        x.wrapping_mul(5),
        x ^ 5,
        x.rotate_left(6),
        x ^ 7,
    ]
}

#[inline(never)]
fn big_param(f: Five, arr: [u32; 8], s: u32) -> u32 {
    f.a ^ f.b
        ^ f.c.wrapping_add(f.d)
        ^ f.e
        ^ arr[(s % 8) as usize]
        ^ arr[((s >> 3) % 8) as usize]
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let (p, q) = pair_ret(input1, input2);
    let (m, n) = mixed_ret(((input1 as u64) << 32) | input2 as u64, input2 | 1);
    let f = five_ret(input1 ^ input2);
    let arr = arr_ret(input2);
    // Two sret results consumed by a big-by-value call keeps the caller's
    // frame buffers live across several call sites.
    let bp = big_param(f, arr, input1.wrapping_add(n));
    p.wrapping_add(q) ^ bp ^ (m as u32) ^ ((m >> 32) as u32)
}
