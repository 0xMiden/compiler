// shortcircuit x calls (campaign 14): `&&` / `||` lattices whose operands
// are side-effecting helper calls (direct and fn-pointer dispatched) that
// write an evaluation log through a `&mut [u32; 8]`, mixed with plain
// compares, negations and a `match` on the lattice result inside a loop
// with a `continue`; the log order and the final booleans are folded into
// the result so any evaluation-order or short-circuit defect shows up.
#[inline(never)]
fn probe(log: &mut [u32; 8], slot: usize, v: u32) -> bool {
    let s = slot & 7;
    log[s] = log[s].wrapping_mul(3).wrapping_add(v | 1);
    v & 3 != 0
}

#[inline(never)]
fn probe_hi(log: &mut [u32; 8], slot: usize, v: u32) -> bool {
    let s = (slot + 1) & 7;
    log[s] = log[s].rotate_left(5) ^ v;
    v >> 31 == 1
}

#[inline(never)]
fn heavy(log: &mut [u32; 8], a: u64, b: u64) -> bool {
    let m = a.wrapping_mul(b | 1);
    log[(m & 7) as usize] = log[(m & 7) as usize].wrapping_add((m >> 32) as u32);
    (m ^ a) & 1 == 1
}

type Pred = fn(&mut [u32; 8], usize, u32) -> bool;

static PREDS: [Pred; 2] = [probe, probe_hi];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut log = [0u32; 8];
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let c1 = probe(&mut log, 0, input1) && PREDS[(input2 & 1) as usize](&mut log, 1, input2)
        || probe(&mut log, 2, input1 ^ input2);
    let c2 = (input1 > input2 || heavy(&mut log, x, y))
        && !(probe(&mut log, 3, input1 >> 3) || PREDS[((input1 >> 2) & 1) as usize](&mut log, 4, input2 >> 5));
    let mut acc = (c1 as u32) | ((c2 as u32) << 1);
    let mut i = 0u32;
    let n = input2 % 9;
    while i < n {
        let s = (i as usize) & 7;
        if probe(&mut log, s, input1.wrapping_add(i)) && heavy(&mut log, x.rotate_left(i), y ^ i as u64) {
            acc = acc.wrapping_mul(5) ^ 0x11;
            i += 1;
            continue;
        }
        let d = match (
            PREDS[(i & 1) as usize](&mut log, s + 2, input2.wrapping_sub(i)),
            input1 & (1 << (i & 31)) != 0 || probe_hi(&mut log, s + 3, input1.rotate_left(i)),
        ) {
            (true, true) => 3,
            (true, false) => 5,
            (false, true) => 7,
            (false, false) => 11,
        };
        acc = acc.wrapping_mul(d) ^ i;
        i += 1;
    }
    let mut fold = acc;
    for (k, v) in log.iter().enumerate() {
        fold = fold.rotate_left(3) ^ v.wrapping_mul(k as u32 | 1);
    }
    fold
}
