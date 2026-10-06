// Passing `--optimize=max` guard for `fir_cordic`: the same 8-tap FIR with
// a 4-iteration CORDIC — the largest rung of the CORDIC ladder that still
// compiles at `--optimize=max` (8 iterations panic; 16 taps need only 2).
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
    while i < 4 {
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
    let mut sig = [0i32; 48];
    let mut i = 0u32;
    while i < 48 {
        sig[i as usize] = ((input1.wrapping_mul(i + 1) ^ input2) as i32) >> 7;
        i += 1;
    }
    let mut fir_acc = 0u32;
    let mut n = 15usize;
    while n < 48 {
        let mut acc = 0i64;
        let mut t = 0usize;
        while t < 8 {
            acc += (TAPS[t] as i64) * (sig[n - t] as i64);
            t += 1;
        }
        let y = sat32((acc + (1 << 15)) >> 16);
        fir_acc = fir_acc.rotate_left(7) ^ y as u32;
        n += 1;
    }
    let cx = (input1 as i32) >> 9;
    let cy = (input2 as i32) >> 9;
    let (ang, mag) = cordic_atan2(cy, cx);
    fir_acc ^ (ang as u32) ^ (mag as u32).rotate_left(8)
}
