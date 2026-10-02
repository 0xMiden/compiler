// Fixed-point DSP (campaign 17, program 8): Q16.16 multiply (i64 product,
// rounded) and divide (i64 dividend, saturated, zero-guarded) chains — a
// Newton reciprocal iteration and a Newton square root cross-checked
// against a digit-by-digit 64-bit integer square root — a 16-tap
// windowed-sinc FIR filter (`.rodata` coefficients, i64 accumulator,
// saturating output) over a 48-sample xorshift signal, and a 16-step
// vectoring CORDIC atan2 / magnitude with a `.rodata` angle table; signed
// arithmetic with rounding at every boundary, every result and check flag
// folded.
const ONE: i32 = 1 << 16;
const HALF_PI: i32 = 102_944;
const CORDIC_K: i32 = 39_797;

static ATAN: [i32; 16] =
    [51472, 30386, 16055, 8150, 4091, 2047, 1024, 512, 256, 128, 64, 32, 16, 8, 4, 2];

static TAPS: [i32; 16] = [
    0, 366, 518, -1084, -3334, 0, 12065, 24277, 24277, 12065, 0, -3334, -1084, 518, 366, 0,
];

fn qmul(a: i32, b: i32) -> i32 {
    let p = (a as i64) * (b as i64);
    ((p + (1 << 15)) >> 16) as i32
}

fn sat32(v: i64) -> i32 {
    if v > i32::MAX as i64 {
        i32::MAX
    } else if v < i32::MIN as i64 {
        i32::MIN
    } else {
        v as i32
    }
}

fn qdiv(a: i32, b: i32) -> i32 {
    if b == 0 {
        return if a < 0 { i32::MIN } else { i32::MAX };
    }
    let n = (a as i64) << 16;
    sat32(n / (b as i64))
}

fn isqrt64(n: u64) -> u64 {
    let mut x = n;
    let mut c = 0u64;
    let mut d = 1u64 << 62;
    while d > n {
        d >>= 2;
    }
    while d != 0 {
        if x >= c + d {
            x -= c + d;
            c = (c >> 1) + d;
        } else {
            c >>= 1;
        }
        d >>= 2;
    }
    c
}

// Q16.16 square root of a non-negative Q16.16 value.
fn qsqrt(x: i32) -> i32 {
    if x <= 0 {
        return 0;
    }
    isqrt64((x as u64) << 16) as i32
}

// Vectoring-mode CORDIC: (angle in Q16.16 radians, magnitude in Q16.16).
fn cordic_atan2(y0: i32, x0: i32) -> (i32, i32) {
    let mut x = x0;
    let mut y = y0;
    let mut angle = 0i32;
    if x < 0 {
        if y >= 0 {
            x = y0;
            y = x0.wrapping_neg();
            angle = HALF_PI;
        } else {
            x = y0.wrapping_neg();
            y = x0;
            angle = HALF_PI.wrapping_neg();
        }
    }
    let mut i = 0u32;
    while i < 16 {
        if y > 0 {
            let xn = x.wrapping_add(y >> i);
            y = y.wrapping_sub(x >> i);
            x = xn;
            angle = angle.wrapping_add(ATAN[i as usize]);
        } else {
            let xn = x.wrapping_sub(y >> i);
            y = y.wrapping_add(x >> i);
            x = xn;
            angle = angle.wrapping_sub(ATAN[i as usize]);
        }
        i += 1;
    }
    (angle, qmul(x, CORDIC_K))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Newton reciprocal of d (Q16.16, d in [0.5, 2)) and its check.
    let d = ((input1 & 0xffff) as i32 + (ONE >> 1)) | 1;
    let mut r = ONE;
    let mut i = 0u32;
    while i < 6 {
        r = qmul(r, (2 * ONE).wrapping_sub(qmul(d, r)));
        i += 1;
    }
    let recip_err = qmul(d, r).wrapping_sub(ONE).wrapping_abs();
    // Square root two ways.
    let s = (input2 >> 4) as i32;
    let root = qsqrt(s);
    let mut newton = if s > ONE { s >> 1 } else { ONE };
    i = 0;
    while i < 20 {
        let q = qdiv(s, newton | 1);
        let next = (newton.wrapping_add(q)) >> 1;
        if next == newton {
            break;
        }
        newton = next;
        i += 1;
    }
    let sqrt_err = root.wrapping_sub(newton).wrapping_abs();
    // A Q16.16 chain: alternating multiply / divide with sign flips.
    let mut chain = (input1 as i32) >> 8;
    let mut k = 0u32;
    while k < 12 {
        let m = ((input2.rotate_left(k * 3) & 0x3ffff) as i32).wrapping_sub(0x1ffff);
        chain = if k & 1 == 0 {
            qmul(chain, m)
        } else {
            qdiv(chain, m >> 2)
        };
        chain = chain.wrapping_add(ONE >> (k & 7));
        k += 1;
    }
    // FIR over a 48-sample signal.
    let mut sig = [0i32; 48];
    let mut x = input1 ^ input2.rotate_left(9) | 1;
    i = 0;
    while i < 48 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        sig[i as usize] = ((x >> 8) as i32) >> 7;
        i += 1;
    }
    let mut fir_acc = 0u32;
    let mut fir_max = 0i32;
    let mut n = 15usize;
    while n < 48 {
        let mut acc = 0i64;
        let mut t = 0usize;
        while t < 16 {
            acc += (TAPS[t] as i64) * (sig[n - t] as i64);
            t += 1;
        }
        let y = sat32((acc + (1 << 15)) >> 16);
        fir_acc = fir_acc.rotate_left(7) ^ y as u32;
        if y.wrapping_abs() > fir_max {
            fir_max = y.wrapping_abs();
        }
        n += 1;
    }
    // CORDIC on input-derived coordinates (both signs, both axes).
    let cx = (input1 as i32) >> 9;
    let cy = (input2 as i32) >> 9;
    let (ang, mag) = cordic_atan2(cy, cx);
    let (ang2, mag2) = cordic_atan2(cx, cy);
    let mag_ref = isqrt64((cx as i64 * cx as i64 + cy as i64 * cy as i64) as u64) as i32;
    let mag_err = (mag >> 16).wrapping_sub(mag_ref).wrapping_abs();
    let flags =
        (recip_err < 4) as u32 | ((sqrt_err < 4) as u32) << 1 | ((mag_err < 64) as u32) << 2;
    let mut h = flags.wrapping_mul(0x9e37_79b9);
    h = h.rotate_left(5) ^ r as u32;
    h = h.rotate_left(5) ^ root as u32;
    h = h.rotate_left(5) ^ newton as u32;
    h = h.rotate_left(5) ^ chain as u32;
    h = h.rotate_left(5) ^ fir_acc;
    h = h.rotate_left(5) ^ fir_max as u32;
    h = h.rotate_left(5) ^ ang as u32;
    h = h.rotate_left(5) ^ mag as u32;
    h = h.rotate_left(5) ^ ang2 as u32;
    h = h.rotate_left(5) ^ mag2 as u32;
    h ^ (mag_err as u32) << 24
}
