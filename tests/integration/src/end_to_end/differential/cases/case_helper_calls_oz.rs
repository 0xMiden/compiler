// Small helpers WITHOUT inline attributes, each called from several sites:
// at -Oz (`--optimize=size-min`) LLVM keeps them as real calls (O2 inlines
// everything), so the entrypoint carries u32 and u64 values live across
// call boundaries — argument marshalling, spill/reload around `exec`, a
// helper with three return sites, and a u64-returning helper.
fn mix(a: u32, b: u32) -> u32 {
    let x = a.rotate_left(5) ^ b;
    let y = x.wrapping_mul(0x27d4_eb2d);
    (y ^ (y >> 15)).wrapping_add(a.rotate_right(11)).wrapping_sub(b >> 3)
}

fn fold64(x: u64, k: u32) -> u64 {
    let r = x.rotate_left(k & 63) ^ (x >> 7).wrapping_add(k as u64);
    r.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (r >> 29)
}

fn pick(sel: u32, x: u32, y: u32) -> u32 {
    if sel & 3 == 0 {
        return x.wrapping_add(y).rotate_left(1);
    }
    if sel & 3 == 1 {
        return (x ^ y).wrapping_mul(3);
    }
    x.wrapping_sub(y) ^ sel.rotate_right(7)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = mix(input1, input2);
    let b = mix(input2, a);
    let w: u64 = ((a as u64) << 32) | (b as u64);
    let w2 = fold64(w, input1);
    let c = mix(w2 as u32, (w2 >> 32) as u32);
    let d = pick(input1, a, b);
    let e = pick(input2, c, d);
    let w3 = fold64(w2 ^ (e as u64), input2);
    let f = pick(a ^ b, e, w3 as u32);
    let g = mix(f, (w3 >> 32) as u32);
    a ^ b ^ c ^ d ^ e ^ f ^ g ^ (w2 as u32) ^ ((w3 >> 32) as u32)
}
