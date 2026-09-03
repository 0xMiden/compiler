// 256-bit big-integer arithmetic (campaign 17, program 5): little-endian
// 8 x u32 limbs — add / sub with carry and borrow chains, a schoolbook
// 256 x 256 -> 512-bit multiply through u64 products, long division by a
// runtime 32-bit divisor with the quotient multiplied back, a Montgomery
// reduction (REDC with a Newton-computed inverse limb) of the product
// modulo the secp256k1 prime, and the same product computed a second way
// on 4 x u64 limbs with u128 multiply-accumulate chains. Every self-check
// (products agree, q * d + r == a, (a + b) - b == a, the reduced value is
// below the modulus, the inverse limb is correct) is a flag folded with the
// limbs of every intermediate into the result.
type U256 = [u32; 8];

static P: U256 = [
    0xffff_fc2f,
    0xffff_fffe,
    0xffff_ffff,
    0xffff_ffff,
    0xffff_ffff,
    0xffff_ffff,
    0xffff_ffff,
    0xffff_ffff,
];

fn add256(a: &U256, b: &U256) -> (U256, u32) {
    let mut r = [0u32; 8];
    let mut carry = 0u64;
    let mut i = 0usize;
    while i < 8 {
        let s = a[i] as u64 + b[i] as u64 + carry;
        r[i] = s as u32;
        carry = s >> 32;
        i += 1;
    }
    (r, carry as u32)
}

fn sub256(a: &U256, b: &U256) -> (U256, u32) {
    let mut r = [0u32; 8];
    let mut borrow = 0u64;
    let mut i = 0usize;
    while i < 8 {
        let d = (a[i] as u64).wrapping_sub(b[i] as u64).wrapping_sub(borrow);
        r[i] = d as u32;
        borrow = d >> 63;
        i += 1;
    }
    (r, borrow as u32)
}

fn mul256(a: &U256, b: &U256) -> [u32; 16] {
    let mut r = [0u32; 16];
    let mut i = 0usize;
    while i < 8 {
        let mut carry = 0u64;
        let mut j = 0usize;
        while j < 8 {
            let t = (a[i] as u64) * (b[j] as u64) + r[i + j] as u64 + carry;
            r[i + j] = t as u32;
            carry = t >> 32;
            j += 1;
        }
        r[i + 8] = carry as u32;
        i += 1;
    }
    r
}

// The same product on 64-bit limbs with u128 multiply-accumulate chains.
fn mul256_u64(a: &[u64; 4], b: &[u64; 4]) -> [u64; 8] {
    let mut r = [0u64; 8];
    let mut i = 0usize;
    while i < 4 {
        let mut carry = 0u128;
        let mut j = 0usize;
        while j < 4 {
            let t = (a[i] as u128) * (b[j] as u128) + r[i + j] as u128 + carry;
            r[i + j] = t as u64;
            carry = t >> 64;
            j += 1;
        }
        r[i + 4] = carry as u64;
        i += 1;
    }
    r
}

fn divrem_u32(a: &U256, d: u32) -> (U256, u32) {
    let mut q = [0u32; 8];
    let mut rem = 0u32;
    let mut i = 8usize;
    while i > 0 {
        i -= 1;
        let cur = ((rem as u64) << 32) | a[i] as u64;
        q[i] = (cur / d as u64) as u32;
        rem = (cur % d as u64) as u32;
    }
    (q, rem)
}

// a * m + addend, returning the low 256 bits and the carry-out limb.
fn mul_u32_add(a: &U256, m: u32, addend: u32) -> (U256, u32) {
    let mut r = [0u32; 8];
    let mut carry = addend as u64;
    let mut i = 0usize;
    while i < 8 {
        let t = (a[i] as u64) * (m as u64) + carry;
        r[i] = t as u32;
        carry = t >> 32;
        i += 1;
    }
    (r, carry as u32)
}

// Limb-wise equality (array `==` would lower to a `memcmp` libcall that
// the guest link does not provide).
fn eq256(a: &U256, b: &U256) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn geq(a: &U256, b: &U256) -> bool {
    let mut i = 8usize;
    while i > 0 {
        i -= 1;
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

// Montgomery reduction of a 512-bit value modulo `n` (R = 2^256).
fn mont_redc(t: &[u32; 16], n: &U256, n_inv: u32) -> U256 {
    let mut t17 = [0u32; 17];
    t17[..16].copy_from_slice(t);
    let mut i = 0usize;
    while i < 8 {
        let m = t17[i].wrapping_mul(n_inv);
        let mut carry = 0u64;
        let mut j = 0usize;
        while j < 8 {
            let s = t17[i + j] as u64 + (m as u64) * (n[j] as u64) + carry;
            t17[i + j] = s as u32;
            carry = s >> 32;
            j += 1;
        }
        let mut k = i + 8;
        while carry != 0 && k < 17 {
            let s = t17[k] as u64 + carry;
            t17[k] = s as u32;
            carry = s >> 32;
            k += 1;
        }
        i += 1;
    }
    let mut r = [0u32; 8];
    r.copy_from_slice(&t17[8..16]);
    if t17[16] != 0 || geq(&r, n) {
        let (d, _) = sub256(&r, n);
        d
    } else {
        r
    }
}

fn xorshift(x: &mut u32) -> u32 {
    let mut v = *x;
    v ^= v << 13;
    v ^= v >> 17;
    v ^= v << 5;
    *x = v;
    v
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut seed = input1 ^ input2.rotate_left(13) ^ 0x6a09_e667 | 1;
    let mut a = [0u32; 8];
    let mut b = [0u32; 8];
    let mut i = 0usize;
    while i < 8 {
        a[i] = xorshift(&mut seed);
        b[i] = xorshift(&mut seed) ^ input2.rotate_left(i as u32 * 3);
        i += 1;
    }
    // Boundary shapes: a near the modulus, b all-ones.
    if input1 & 1 == 1 {
        a = P;
        a[0] = a[0].wrapping_sub(1 + (input2 & 0xff));
    }
    if input2 & 1 == 1 {
        b = [0xffff_ffff; 8];
        b[7] ^= input1 >> 8;
    }
    // Newton inverse of the low modulus limb: n_inv = -P[0]^-1 mod 2^32.
    let n0 = P[0] ^ ((input1 & 0) << 1);
    let mut inv = n0;
    i = 0;
    while i < 5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub(n0.wrapping_mul(inv)));
        i += 1;
    }
    let inv_ok = (n0.wrapping_mul(inv) == 1) as u32;
    let n_inv = inv.wrapping_neg();

    let prod = mul256(&a, &b);
    let a64 = [
        a[0] as u64 | (a[1] as u64) << 32,
        a[2] as u64 | (a[3] as u64) << 32,
        a[4] as u64 | (a[5] as u64) << 32,
        a[6] as u64 | (a[7] as u64) << 32,
    ];
    let b64 = [
        b[0] as u64 | (b[1] as u64) << 32,
        b[2] as u64 | (b[3] as u64) << 32,
        b[4] as u64 | (b[5] as u64) << 32,
        b[6] as u64 | (b[7] as u64) << 32,
    ];
    let prod64 = mul256_u64(&a64, &b64);
    let mut same = 1u32;
    i = 0;
    while i < 8 {
        let lo = prod[2 * i] as u64 | (prod[2 * i + 1] as u64) << 32;
        if lo != prod64[i] {
            same = 0;
        }
        i += 1;
    }

    let d = input1.rotate_left(7) | 1;
    let (q, r) = divrem_u32(&a, d);
    let (back, back_carry) = mul_u32_add(&q, d, r);
    let div_ok = (eq256(&back, &a) && back_carry == 0) as u32;

    let (s, carry) = add256(&a, &b);
    let (s2, borrow) = sub256(&s, &b);
    let add_ok = (eq256(&s2, &a) && carry == borrow) as u32;

    let m1 = mont_redc(&prod, &P, n_inv);
    let prod_ba = mul256(&b, &a);
    let m2 = mont_redc(&prod_ba, &P, n_inv);
    let mont_ok = (eq256(&m1, &m2) && !geq(&m1, &P)) as u32;

    let flags = same | div_ok << 1 | add_ok << 2 | mont_ok << 3 | inv_ok << 4;
    let mut acc = flags.wrapping_mul(0x9e37_79b9) ^ r ^ carry << 5 ^ borrow << 6;
    i = 0;
    while i < 16 {
        acc = acc.rotate_left(7) ^ prod[i];
        if i < 8 {
            acc = acc
                .rotate_left(3)
                .wrapping_add(q[i] ^ s[i].rotate_left(11) ^ m1[i].rotate_left(19));
        }
        i += 1;
    }
    acc
}
