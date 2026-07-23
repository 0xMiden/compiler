// SCALE DIMENSION: procedure count and call depth. Thirty #[inline(never)]
// helpers: a 20-deep non-recursive call chain (c01 -> c02 -> ... -> c20) where
// every level also fans out to one of ten leaf helpers — 30 procedure digests
// in the MAST forest and a 21+-frame VM call stack at runtime. All signatures
// are 2-3 u32s, far under the 16-felt cap.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    c01(input1 | 1, input2 ^ 0x9e37_79b9)
}

#[inline(never)]
fn f1(x: u32, y: u32) -> u32 {
    x.rotate_left(1).wrapping_add(y ^ 0x0101_0101)
}
#[inline(never)]
fn f2(x: u32, y: u32) -> u32 {
    y.rotate_right(2).wrapping_sub(x).wrapping_mul(0x0202_0203)
}
#[inline(never)]
fn f3(x: u32, y: u32) -> u32 {
    (x ^ y).wrapping_mul(0x0303_0305) | 1
}
#[inline(never)]
fn f4(x: u32, y: u32) -> u32 {
    x.wrapping_sub(y.rotate_left(4)) ^ 0x0404_0407
}
#[inline(never)]
fn f5(x: u32, y: u32) -> u32 {
    (x | 0x0505_0509).wrapping_add(y.rotate_right(5))
}
#[inline(never)]
fn f6(x: u32, y: u32) -> u32 {
    (x & y).wrapping_mul(0x0606_060b).rotate_left(6)
}
#[inline(never)]
fn f7(x: u32, y: u32) -> u32 {
    (x ^ y).rotate_left(7).wrapping_sub(0x0707_070d)
}
#[inline(never)]
fn f8(x: u32, y: u32) -> u32 {
    y.wrapping_mul(x | 8).wrapping_add(0x0808_080f)
}
#[inline(never)]
fn f9(x: u32, y: u32) -> u32 {
    x.rotate_right(9) ^ y.wrapping_add(0x0909_0911)
}
#[inline(never)]
fn f10(x: u32, y: u32) -> u32 {
    (x.wrapping_add(y) | 3).wrapping_mul(0x0a0a_0a13)
}

#[inline(never)]
fn c01(x: u32, y: u32) -> u32 {
    c02(x.rotate_left(1) ^ y, y.wrapping_add(0x0001), x).wrapping_add(f1(x, y))
}
#[inline(never)]
fn c02(x: u32, y: u32, z: u32) -> u32 {
    c03(x.rotate_left(2) ^ z, y.wrapping_add(0x0002)).wrapping_sub(f2(x, y))
}
#[inline(never)]
fn c03(x: u32, y: u32) -> u32 {
    c04(x.rotate_left(3) ^ y, y.wrapping_add(0x0003), x) ^ f3(x, y)
}
#[inline(never)]
fn c04(x: u32, y: u32, z: u32) -> u32 {
    c05(x.rotate_left(4) ^ z, y.wrapping_add(0x0004)).wrapping_add(f4(x, y))
}
#[inline(never)]
fn c05(x: u32, y: u32) -> u32 {
    c06(x.rotate_left(5) ^ y, y.wrapping_add(0x0005)).wrapping_mul(f5(x, y) | 1)
}
#[inline(never)]
fn c06(x: u32, y: u32) -> u32 {
    c07(x.rotate_left(6) ^ y, y.wrapping_add(0x0006), x).wrapping_sub(f6(x, y))
}
#[inline(never)]
fn c07(x: u32, y: u32, z: u32) -> u32 {
    c08(x.rotate_left(7) ^ z, y.wrapping_add(0x0007)) ^ f7(x, y)
}
#[inline(never)]
fn c08(x: u32, y: u32) -> u32 {
    c09(x.rotate_left(8) ^ y, y.wrapping_add(0x0008)).wrapping_add(f8(x, y))
}
#[inline(never)]
fn c09(x: u32, y: u32) -> u32 {
    c10(x.rotate_left(9) ^ y, y.wrapping_add(0x0009), x).wrapping_sub(f9(x, y))
}
#[inline(never)]
fn c10(x: u32, y: u32, z: u32) -> u32 {
    c11(x.rotate_left(10) ^ z, y.wrapping_add(0x000a)) ^ f10(x, y)
}
#[inline(never)]
fn c11(x: u32, y: u32) -> u32 {
    c12(x.rotate_left(11) ^ y, y.wrapping_add(0x000b)).wrapping_add(f1(y, x))
}
#[inline(never)]
fn c12(x: u32, y: u32) -> u32 {
    c13(x.rotate_left(12) ^ y, y.wrapping_add(0x000c), x).wrapping_sub(f2(y, x))
}
#[inline(never)]
fn c13(x: u32, y: u32, z: u32) -> u32 {
    c14(x.rotate_left(13) ^ z, y.wrapping_add(0x000d)) ^ f3(y, x)
}
#[inline(never)]
fn c14(x: u32, y: u32) -> u32 {
    c15(x.rotate_left(14) ^ y, y.wrapping_add(0x000e)).wrapping_add(f4(y, x))
}
#[inline(never)]
fn c15(x: u32, y: u32) -> u32 {
    c16(x.rotate_left(15) ^ y, y.wrapping_add(0x000f), x).wrapping_mul(f5(y, x) | 1)
}
#[inline(never)]
fn c16(x: u32, y: u32, z: u32) -> u32 {
    c17(x.rotate_left(16) ^ z, y.wrapping_add(0x0010)).wrapping_sub(f6(y, x))
}
#[inline(never)]
fn c17(x: u32, y: u32) -> u32 {
    c18(x.rotate_left(17) ^ y, y.wrapping_add(0x0011)) ^ f7(y, x)
}
#[inline(never)]
fn c18(x: u32, y: u32) -> u32 {
    c19(x.rotate_left(18) ^ y, y.wrapping_add(0x0012), x).wrapping_add(f8(y, x))
}
#[inline(never)]
fn c19(x: u32, y: u32, z: u32) -> u32 {
    c20(x.rotate_left(19) ^ z, y.wrapping_add(0x0013)).wrapping_sub(f9(y, x))
}
#[inline(never)]
fn c20(x: u32, y: u32) -> u32 {
    f10(x, y).wrapping_add(f1(x.rotate_left(20), y)) ^ f2(x, y ^ 0x0014)
}
