// Passing sibling of `case_invariant_args_noreturn.rs` (campaign 28, W4): the
// same program with SIX mixing steps instead of seven, which LLVM unrolls into
// the block loop -- so cfg-to-scf sees one loop instead of a nest and no
// payload column carries poison.
const IV: [u64; 2] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
];
const SIGMA: [[u8; 12]; 1] = [
    [0, 5, 10, 3, 8, 1, 6, 11, 4, 9, 2, 7],
];
const GIDX: [[u8; 4]; 6] = [
    [0, 1, 2, 3],
    [1, 0, 3, 2],
    [0, 1, 2, 3],
    [1, 0, 3, 2],
    [0, 1, 2, 3],
    [1, 0, 3, 2],
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut m = [0u64; 12];
    let mut x = (input1 as u64) | 1;
    let mut i = 0usize;
    while i < 12 {
        x ^= x << 13;
        x ^= x >> 7;
        m[i] = x ^ ((input2 as u64) << 16);
        i += 1;
    }
    let mut h = IV;
    h[0] ^= 0x0101_0000 ^ 64;
    let blocks = 1 + (input2 % 2) as usize;
    let mut blk = 0usize;
    while blk < blocks {
        let mut v = [0u64; 4];
        let mut j = 0usize;
        while j < 2 {
            v[j] = h[j];
            v[j + 2] = IV[j];
            j += 1;
        }
        v[1] ^= (128 * (blk as u64 + 1)) ^ (input1 as u64);
        let mut r = 0usize;
        while r < 1 {
            let mut gi = 0usize;
            while gi < 6 {
                let ia = GIDX[gi][0] as usize;
                let ib = GIDX[gi][1] as usize;
                let ic = GIDX[gi][2] as usize;
                let id = GIDX[gi][3] as usize;
                let mx = m[SIGMA[r][2 * gi] as usize];
                let my = m[SIGMA[r][2 * gi + 1] as usize];
                let mut va = v[ia];
                let mut vb = v[ib];
                let mut vc = v[ic];
                let mut vd = v[id];
                va = va.wrapping_add(vb).wrapping_add(mx);
                vd = (vd ^ va).rotate_right(32);
                vc = vc.wrapping_add(vd);
                vb = (vb ^ vc).rotate_right(24);
                va = va.wrapping_add(vb).wrapping_add(my);
                vd = (vd ^ va).rotate_right(16);
                vc = vc.wrapping_add(vd);
                vb = (vb ^ vc).rotate_right(63);
                v[ia] = va;
                v[ib] = vb;
                v[ic] = vc;
                v[id] = vd;
                gi += 1;
            }
            r += 1;
        }
        let mut k = 0usize;
        while k < 2 {
            h[k] ^= v[k] ^ v[k + 2];
            k += 1;
        }
        blk += 1;
    }
    let mut out = 0u64;
    let mut q = 0usize;
    while q < 2 {
        let d = h[q];
        out = (out ^ d).rotate_right(32);
        out = out.wrapping_add(d.rotate_right(24) ^ d.rotate_right(16));
        out ^= d.rotate_right(63);
        q += 1;
    }
    (out as u32) ^ ((out >> 32) as u32)
}
