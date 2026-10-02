// Indirect dispatch under operand pressure (campaign 14): six u64 values
// kept in wasm locals (each an argument of the dispatch AND used after it)
// are live across a 7-u64 fn-pointer dispatch (14 argument felts + the
// table index = 15 of the 16-felt window) inside a loop whose table index
// is loop-carried; then `dyn Trait` objects holding u64 fields are
// dispatched through a method returning a u128 (return-area pointer +
// receiver + four u64) under the same live state, twice, with the second
// receiver chosen by the first result. Six is the largest local count that
// compiles: seven hits the `indirect_spill` panic (the spill analysis does
// not see `hir.exec_indirect` arguments; see tests/calls.rs). Generated
// from scratch/c14gen_dp.py (N = 6) plus the dyn tail.
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

trait Mix {
    fn mix(&self, a: u64, b: u64, c: u64, d: u64) -> u128;
}

struct Lin(u64, u64);
struct Rot(u64, u32);

impl Mix for Lin {
    #[inline(never)]
    fn mix(&self, a: u64, b: u64, c: u64, d: u64) -> u128 {
        let lo = a.wrapping_mul(self.0 | 1) ^ b.wrapping_add(self.1);
        let hi = c ^ d.rotate_left(9) ^ self.0;
        ((hi as u128) << 64) | lo as u128
    }
}

impl Mix for Rot {
    #[inline(never)]
    fn mix(&self, a: u64, b: u64, c: u64, d: u64) -> u128 {
        let lo = a.rotate_left(self.1 & 63) ^ b ^ self.0;
        let hi = c.wrapping_sub(d).rotate_right(self.1 >> 26);
        (((hi as u128) << 64) | lo as u128).wrapping_mul(0x0000_0000_0000_0003_0000_0000_0000_0005)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let v0 = x.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ y.rotate_left(21);
    let v1 = y.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ x.rotate_left(23);
    let v2 = x.wrapping_mul(0x94d0_49bb_1331_11eb) ^ y.rotate_left(25);
    let v3 = y.wrapping_mul(0xd6e8_feb8_6659_fd93) ^ x.rotate_left(27);
    let v4 = x.wrapping_mul(0xa076_1d64_78bd_642f) ^ y.rotate_left(29);
    let v5 = y.wrapping_mul(0xe703_7ed1_a0b4_28db) ^ x.rotate_left(31);
    let lin = Lin(x, y);
    let rot = Rot(x ^ y, input1);
    let objs: [&dyn Mix; 2] = [&lin, &rot];
    let mut idx = (input1 & 1) as usize;
    let mut acc = x;
    let n = (input2 % 7).wrapping_add(1);
    let mut i = 0u32;
    while i < n {
        let f = WIDES[idx];
        let r = f(v0, v1, v2, v3 ^ acc, v4, v5.wrapping_add(i as u64), v0);
        acc = acc.rotate_left(19) ^ r ^ v0 ^ v1 ^ v2;
        idx = (r & 1) as usize;
        i = i.wrapping_add(1);
    }
    let o = objs[(acc & 1) as usize];
    let m = o.mix(v5, v4, v3, acc);
    let o2 = objs[((acc >> 7) & 1) as usize];
    let m2 = o2.mix(v0 ^ (m as u64), v1, v2, v3);
    let z = (m as u64)
        ^ ((m >> 64) as u64).rotate_left(16)
        ^ (m2 as u64).rotate_left(18)
        ^ ((m2 >> 64) as u64)
        ^ v0.rotate_left(2)
        ^ v1.rotate_left(4)
        ^ v2.rotate_left(6)
        ^ v3.rotate_left(8)
        ^ v4.rotate_left(10)
        ^ v5.rotate_left(12)
        ^ acc;
    (z as u32) ^ ((z >> 32) as u32) ^ (idx as u32)
}
