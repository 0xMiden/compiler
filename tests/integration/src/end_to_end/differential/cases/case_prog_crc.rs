// Checksums and shift-register generators (campaign 17, program 12): a
// table-driven CRC-32 (IEEE, `const fn`-built 256-entry `.rodata` table)
// cross-checked against a bitwise CRC-32, a bitwise CRC-16/CCITT, a CRC-8
// through a table built at runtime in a stack array, Adler-32 (modular
// arithmetic on u32), a 32-bit Galois LFSR and a 16-bit Fibonacci LFSR,
// all over an xorshift-filled 64-byte buffer of runtime length; every
// checksum, both generators' final states and the table-vs-bitwise flag
// are folded into the result.
const fn gen_crc32_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC32_TABLE: [u32; 256] = gen_crc32_table();

fn crc32_table(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    let mut i = 0usize;
    while i < data.len() {
        c = CRC32_TABLE[((c ^ data[i] as u32) & 0xff) as usize] ^ (c >> 8);
        i += 1;
    }
    !c
}

fn crc32_bitwise(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    let mut i = 0usize;
    while i < data.len() {
        c ^= data[i] as u32;
        let mut k = 0u32;
        while k < 8 {
            let mask = 0u32.wrapping_sub(c & 1);
            c = (c >> 1) ^ (0xedb8_8320 & mask);
            k += 1;
        }
        i += 1;
    }
    !c
}

fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut c = 0xffffu16;
    let mut i = 0usize;
    while i < data.len() {
        c ^= (data[i] as u16) << 8;
        let mut k = 0u32;
        while k < 8 {
            c = if c & 0x8000 != 0 {
                (c << 1) ^ 0x1021
            } else {
                c << 1
            };
            k += 1;
        }
        i += 1;
    }
    c
}

fn crc8(data: &[u8], poly: u8) -> u8 {
    let mut table = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u8;
        let mut k = 0u32;
        while k < 8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ poly
            } else {
                c << 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    let mut c = 0u8;
    i = 0;
    while i < data.len() {
        c = table[(c ^ data[i]) as usize];
        i += 1;
    }
    c
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    let mut i = 0usize;
    while i < data.len() {
        a = (a + data[i] as u32) % 65521;
        b = (b + a) % 65521;
        i += 1;
    }
    (b << 16) | a
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u8; 64];
    let mut x = input1 ^ input2.rotate_left(19) ^ 0x1b87_3593 | 1;
    let mut i = 0usize;
    while i < 64 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        buf[i] = (x >> 11) as u8;
        i += 1;
    }
    let len = (input2 % 65) as usize;
    let data = &buf[..len];
    let c32t = crc32_table(data);
    let c32b = crc32_bitwise(data);
    let c16 = crc16_ccitt(data);
    let c8 = crc8(data, 0x07 | (input1 as u8 & 0x30));
    let ad = adler32(data);
    // Galois LFSR (taps 0x8020_0003) clocked an input-derived number of
    // times, and a 16-bit Fibonacci LFSR (taps 16, 14, 13, 11).
    let mut g = input1 | 1;
    let mut fib = (input2 as u16) | 1;
    let mut gsum = 0u32;
    let n = 16 + input1 % 48;
    i = 0;
    while (i as u32) < n {
        let lsb = g & 1;
        g >>= 1;
        g ^= 0x8020_0003 & 0u32.wrapping_sub(lsb);
        let bit = (fib ^ (fib >> 2) ^ (fib >> 3) ^ (fib >> 5)) & 1;
        fib = (fib >> 1) | (bit << 15);
        gsum = gsum.wrapping_add(g ^ fib as u32);
        i += 1;
    }
    let same = (c32t == c32b) as u32;
    c32t ^ (c16 as u32).rotate_left(11)
        ^ (c8 as u32) << 24
        ^ ad.rotate_left(5)
        ^ g.rotate_left(17)
        ^ gsum
        ^ same << 31
}
