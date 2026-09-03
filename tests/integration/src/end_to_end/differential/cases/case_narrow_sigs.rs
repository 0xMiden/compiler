// ext_chains x calls (campaign 14): narrow signed and unsigned values
// crossing call boundaries — helpers with i8 / i16 / u8 / u16 / bool
// parameters and i8 / i16 / u8 / u16 / bool / i32 results (sign- and
// zero-extension at the caller, truncation on return), chained through a
// loop whose carried values are narrow, with parameters at MIN / -1 / MAX
// reached from the inputs and `as` widenings to i32 / i64 / u64 of every
// result, plus a narrow-typed fn-pointer dispatch.
#[inline(never)]
fn s8(a: i8, b: u8, c: i16) -> i8 {
    a.wrapping_add((b >> 1) as i8) ^ (c >> 8) as i8
}

#[inline(never)]
fn s16(a: i16, b: i8, c: u16, d: bool) -> i16 {
    let v = a.wrapping_mul(b as i16) ^ (c as i16).rotate_left(3);
    if d { v.wrapping_neg() } else { v }
}

#[inline(never)]
fn u8f(a: u8, b: i8, c: u16) -> u8 {
    a.wrapping_mul(b as u8) ^ (c >> 5) as u8
}

#[inline(never)]
fn u16f(a: u16, b: i16, c: u8, d: bool) -> u16 {
    a.wrapping_sub(b as u16).rotate_right(c as u32 & 15) ^ (d as u16) << 15
}

#[inline(never)]
fn cmp(a: i8, b: i16, c: i32) -> bool {
    (a as i32) < (b as i32) && (b as i32).wrapping_mul(3) >= c
}

#[inline(never)]
fn widen(a: i8, b: i16, c: u8, d: u16) -> i32 {
    (a as i32).wrapping_mul(b as i32) ^ ((c as i32) << 16) ^ (d as i32).wrapping_neg()
}

type Narrow = fn(i8, u8, i16) -> i8;

static NARROWS: [Narrow; 2] = [s8, s8b];

#[inline(never)]
fn s8b(a: i8, b: u8, c: i16) -> i8 {
    (a ^ b as i8).wrapping_sub((c & 0x7f) as i8)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = input1 as i8;
    let mut b = (input2 >> 8) as u8;
    let mut c = (input1 >> 16) as i16;
    let mut d = input2 as u16;
    let mut acc = 0i64;
    let n = input2 % 7 + 1;
    let mut i = 0u32;
    while i < n {
        let r8 = s8(a, b, c);
        let r16 = s16(c, r8, d, cmp(a, c, input1 as i32));
        let ru8 = u8f(b, a, d);
        let ru16 = u16f(d, r16, ru8, (i & 1) == 1);
        let f = NARROWS[(ru8 & 1) as usize];
        let rd = f(r8.wrapping_add(i as i8), ru8, r16);
        acc = acc
            .wrapping_add(r8 as i64)
            .wrapping_mul(3)
            .wrapping_add(r16 as i64)
            .wrapping_sub(ru8 as i64)
            .wrapping_add((ru16 as i64) << 3)
            .wrapping_add(rd as i64)
            .wrapping_add(widen(rd, r16, ru8, ru16) as i64);
        a = rd ^ i8::MIN.wrapping_add((i & 1) as i8);
        b = ru8.wrapping_add(0xff);
        c = r16 ^ i16::MAX;
        d = ru16.wrapping_add(u16::MAX);
        i += 1;
    }
    let tail = widen(a, c, b, d) as i64 ^ (s16(i16::MIN, -1, u16::MAX, true) as i64) ^ (u8f(u8::MAX, i8::MIN, 0) as i64);
    let z = acc ^ tail.wrapping_mul(0x9e37_79b9);
    (z as u32) ^ ((z >> 32) as u32) ^ (a as u32) ^ ((b as u32) << 8) ^ ((c as u32) << 16) ^ (d as u32)
}
