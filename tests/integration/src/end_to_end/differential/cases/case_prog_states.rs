// Framing state machine (campaign 21, cliff shape: in-loop wide dispatch with
// u64 state).  A sixteen-state serial-protocol parser consumes a 48-byte
// stream one byte at a time; the driver loop dispatches on the current state
// with a `match` over all sixteen states (three of them share the resync
// body), and every state arm updates the same eight u64 accumulators — frame
// hash, payload hash, CRC accumulator, escape fold, byte/frame counters, an
// error mask and a timing fold — which are combined in ONE watchdog
// expression at the bottom of the loop and folded into the result after it.
// The rotate constants 3, 11, 23, 31, 41 and 55 are shared by the stream
// builder, the state arms and the final fold.
const T0: u32 = 3;
const T1: u32 = 11;
const T2: u32 = 23;
const T3: u32 = 31;
const T4: u32 = 41;
const T5: u32 = 55;

const POLY: u64 = 0x42f0_e1eb_a9ea_3693;

fn stream_of(seed: u32, mode: u32) -> [u8; 48] {
    let mut s = [0u8; 48];
    let mut x = seed | 1;
    let mut i = 0usize;
    while i < 48 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        s[i] = (x >> 11) as u8;
        i += 1;
    }
    // Plant frame headers so the machine actually walks its states.
    s[0] = 0x7e;
    s[1] = 0xa5;
    s[2] = 4 + (mode & 3) as u8;
    s[3] = (mode & 7) as u8;
    s[16] = 0x7e;
    s[17] = 0xa5;
    s[18] = 6;
    s[19] = 2;
    if mode & 8 == 8 {
        s[9] = 0x5c; // escape byte inside the first payload
    }
    s[32] = 0x7e;
    s[33] = if mode & 4 == 4 { 0x00 } else { 0xa5 };
    s[34] = 3;
    s
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let stream = stream_of(input1, input2);

    let mut fhash = POLY.rotate_left(T0) ^ (input1 as u64);
    let mut phash = POLY.rotate_left(T1) ^ (input2 as u64);
    let mut crc = 0xffff_ffff_ffff_ffffu64;
    let mut esc = POLY.rotate_left(T2);
    let mut bytes = 0u64;
    let mut frames = 0u64;
    let mut errors = 0u64;
    let mut timing = POLY.rotate_left(T3);

    let mut state = 0u32;
    let mut remaining = 0u32;
    let mut i = 0usize;
    while i < 48 {
        let b = stream[i] as u64;
        i += 1;
        bytes = bytes.wrapping_add(1);
        match state {
            0 => {
                // IDLE: wait for the frame delimiter.
                if b == 0x7e {
                    state = 1;
                    fhash = fhash.rotate_left(T0) ^ b;
                } else {
                    timing = timing.wrapping_add(b.rotate_left(T1));
                }
            }
            1 => {
                // SYNC: the second header byte.
                if b == 0xa5 {
                    state = 2;
                } else {
                    errors |= 1;
                    state = 12;
                }
                fhash = fhash.wrapping_mul(POLY) ^ b.rotate_left(T2);
            }
            2 => {
                // LEN
                remaining = (b as u32) & 15;
                state = if remaining == 0 { 9 } else { 3 };
                crc ^= b.rotate_left(T3);
            }
            3 => {
                // TYPE
                state = if b & 0x80 == 0 { 4 } else { 5 };
                phash = phash.wrapping_add(b.rotate_left(T4));
            }
            4 => {
                // PAYLOAD (unescaped)
                if b == 0x5c {
                    state = 6;
                } else {
                    phash = (phash ^ b).wrapping_mul(POLY).rotate_left(T0);
                    remaining = remaining.saturating_sub(1);
                    if remaining == 0 {
                        state = 7;
                    }
                }
            }
            5 => {
                // PAYLOAD (typed): the same body, plus a type fold.
                phash = (phash ^ b.rotate_left(T1)).wrapping_mul(POLY);
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    state = 7;
                }
            }
            6 => {
                // ESCAPE
                esc = esc.wrapping_add((b ^ 0x20).rotate_left(T2));
                phash ^= (b ^ 0x20).rotate_left(T3);
                remaining = remaining.saturating_sub(1);
                state = if remaining == 0 { 7 } else { 4 };
            }
            7 => {
                // CRC low
                crc = (crc ^ b).wrapping_mul(POLY).rotate_left(T4);
                state = 8;
            }
            8 => {
                // CRC high
                crc = (crc ^ b.rotate_left(T5)).wrapping_mul(POLY);
                state = 9;
            }
            9 => {
                // CHECK
                if (crc ^ phash) & 0xff == 0 {
                    frames = frames.wrapping_add(1);
                    state = 10;
                } else {
                    errors |= 2;
                    state = 11;
                }
            }
            10 => {
                // ACK
                fhash = fhash.wrapping_add(crc.rotate_left(T0));
                frames = frames.wrapping_add(1);
                state = 0;
            }
            11 => {
                // NAK
                errors |= 4;
                fhash ^= phash.rotate_left(T1);
                state = 13;
            }
            12 | 13 | 14 => {
                // RESYNC / FLUSH / HOLD share one body: skip to the next
                // delimiter.
                if b == 0x7e {
                    state = 1;
                    esc ^= esc.rotate_left(T2);
                } else {
                    timing = timing.wrapping_sub(b.rotate_left(T3));
                }
            }
            _ => {
                // DONE and every out-of-range state.
                errors |= 8;
                state = 0;
            }
        }

        // Watchdog: every accumulator is live at this point.
        let watch = (fhash ^ phash.rotate_left(T0))
            .wrapping_add(crc ^ esc.rotate_left(T1))
            .wrapping_mul(bytes | 1)
            ^ (frames.rotate_left(T2))
            ^ errors.wrapping_add(timing.rotate_left(T3));
        timing = timing.wrapping_add(watch.rotate_left(T4));
        if watch & 0xff_0000 == 0x42_0000 {
            state = 14;
        }
    }

    let mut out = fhash.rotate_left(T0) ^ phash.rotate_left(T1);
    out = out.wrapping_add(crc.rotate_left(T2));
    out ^= esc.rotate_left(T3);
    out = out.wrapping_sub(bytes.rotate_left(T4));
    out ^= frames.rotate_left(T5) ^ errors;
    out = out.wrapping_mul(timing | 1);
    out ^= (state as u64).wrapping_mul(POLY);
    (out as u32) ^ ((out >> 32) as u32)
}
