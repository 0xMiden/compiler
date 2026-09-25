// chain_window x calls (campaign 14): six masked shift counts derived from
// input1 are shared by the code BEFORE a pinned direct 7-u64 call, passed
// to a 6-u32 + u64 helper, used again between a fn-pointer dispatch and
// after it — CSE-merged count bands live across direct and indirect call
// boundaries (Copy-constrained u32 operands under the callee's argument
// window). The dispatch takes plain locals only: computing two of its
// arguments in place (`w.rotate_left(c5)`, `x.rotate_right(c0)`) is the
// `indirect_spill_args` panic (tests/calls.rs).
type Wide = fn(u64, u64, u64, u64, u64, u64, u64) -> u64;

#[inline(never)]
fn w_fold(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64) -> u64 {
    a.wrapping_add(b).wrapping_mul(c | 1).wrapping_sub(d).rotate_left((e & 63) as u32) ^ f.wrapping_add(g)
}

#[inline(never)]
fn w_zip(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64) -> u64 {
    (a ^ b.rotate_right(17)).wrapping_add(c.wrapping_mul(c)).wrapping_add(d >> 3).wrapping_add(e << 5).wrapping_add(f ^ g.swap_bytes())
}

#[inline(never)]
fn counts(v: u64, c0: u32, c1: u32, c2: u32, c3: u32, c4: u32, c5: u32) -> u64 {
    v.rotate_left(c0) ^ v.rotate_right(c1) ^ (v << c2) ^ (v >> c3) ^ v.rotate_left(c4).wrapping_mul(c5 as u64 | 1)
}

static WIDES: [Wide; 2] = [w_fold, w_zip];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let c0 = input1 & 63;
    let c1 = (input1 >> 6) & 63;
    let c2 = (input1 >> 12) & 63;
    let c3 = (input1 >> 18) & 63;
    let c4 = (input1 >> 24) & 63;
    let c5 = (input2 >> 3) & 63;
    let v = x.rotate_left(c0) ^ y.rotate_left(c1) ^ x.rotate_right(c2);
    let w = y.rotate_left(c3) ^ x.rotate_right(c4) ^ v.rotate_left(c5);
    let r = w_fold(v, w, x, y, v ^ w, x.rotate_left(c0), y.rotate_left(c1));
    let s = counts(r, c0, c1, c2, c3, c4, c5);
    let t = s.rotate_left(c2) ^ r.rotate_right(c3) ^ v.rotate_left(c4);
    let f = WIDES[((s >> 9) & 1) as usize];
    let u = f(t, s, r, w, v, x, y);
    let z = u.rotate_left(c1) ^ t.rotate_right(c2) ^ s.rotate_left(c3) ^ r.rotate_left(c4) ^ u.rotate_right(c5) ^ v.rotate_left(c0);
    (z as u32) ^ ((z >> 32) as u32)
}
