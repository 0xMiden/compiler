// F18 LADDER, one rung past the old boundary (campaign 31, 2026-09-17). F18
// was "the coercion folders mutate the operand constant's attribute in
// place", and campaign 28 measured its reach as "only the SIGNED folder has a
// plain-Rust producer": `Sext::fold` was the one that could reach an
// attribute a plain `i64` use still held. With the folders fixed, this case
// makes ALL THREE coercion folders compete for the same literal in one
// function -- `Sext::fold` through an `i128` widening multiply, `Zext::fold`
// through a `u128` one, `Trunc::fold` through a rotate count that
// `mask_movement_count` truncates to `u32` -- and then consumes the literal
// again through four plain uses at three widths (`i64` multiply and add,
// `u64` multiply, `u32` add), with BOTH words of each wide product used so a
// wrong-width push cannot hide in a dropped limb.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = (((input1 as u64) << 32) | (input2 as u64)) as i64;
    let b = (((input2 as u64) << 32) | (input1 as u64)) as i64;
    let u = ((input1 as u64) << 32) | (input2 as u64) | 1;
    let v = ((input2 as u64) << 32) | (input1 as u64) | 3;

    // Sext::fold — the signed widening multiply, both words consumed.
    let sp = (a as i128).wrapping_mul(10);
    let sp_hi = (sp >> 64) as i64;
    let sp_lo = sp as i64;

    // Zext::fold — the unsigned widening multiply, both words consumed.
    let up = (u as u128).wrapping_mul(10);
    let up_hi = (up >> 64) as u64;
    let up_lo = up as u64;

    // Trunc::fold — the same literal as a 64-bit rotate count.
    let rot = v.rotate_left(10) ^ v.rotate_right(10);

    // Plain uses of the same literal at three widths.
    let p1 = b.wrapping_mul(10);
    let p2 = b.wrapping_add(10);
    let p3 = u.wrapping_mul(10);
    let p4 = input1.wrapping_add(10);

    let r = sp_hi
        ^ sp_lo
        ^ (up_hi as i64)
        ^ (up_lo as i64)
        ^ (rot as i64)
        ^ p1
        ^ p2
        ^ (p3 as i64)
        ^ (p4 as i64);
    (r as u32) ^ ((r >> 32) as u32)
}
