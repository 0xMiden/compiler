// nest x calls (campaign 14): a five-level loop nest (levels 1, 3 and 5
// zero-trip-capable) with a pinned helper call at EVERY level whose result
// decides that level's exit (`break` of the level, labeled break of level
// 1 from level 2, a same-level `continue` at level 3, early `return` from
// level 4, `break` at level 5), so the lifted exit dispatch threads call
// results through five levels of region result columns; the carried u64
// crosses every call. A `continue` of an OUTER level from an inner loop
// that contains a call is the `nest_continue` panic (tests/compose.rs).
#[inline(never)]
fn probe(level: u32, v: u64, i: u32) -> u32 {
    let m = v.wrapping_mul(0x9e37_79b9_7f4a_7c15 ^ level as u64).rotate_left(i & 63);
    (m >> 59) as u32
}

#[inline(never)]
fn mix(v: u64, t: u32) -> u64 {
    v.rotate_left(t & 63) ^ (t as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut v = (input1 as u64) << 32 | input2 as u64;
    let n1 = input2 % 5;
    let n2 = input1 % 3 + 1;
    let n3 = (input1 >> 4) % 4;
    let n4 = (input2 >> 4) % 3 + 1;
    let n5 = (input1 >> 8) % 3;
    let mut tag = 0u32;
    let mut i1 = 0;
    'l1: while i1 < n1 {
        let p1 = probe(1, v, i1);
        if p1 == 31 {
            tag |= 1;
            break;
        }
        v = mix(v, p1);
        let mut i2 = 0;
        while i2 < n2 {
            let p2 = probe(2, v, i2);
            if p2 == 30 {
                tag |= 2;
                break 'l1;
            }
            v = mix(v, p2);
            let mut i3 = 0;
            'l3: while i3 < n3 {
                let p3 = probe(3, v, i3);
                if p3 & 7 == 7 {
                    tag |= 4;
                    i3 += 1;
                    continue;
                }
                v = mix(v, p3);
                let mut i4 = 0;
                while i4 < n4 {
                    let p4 = probe(4, v, i4);
                    if p4 == 29 {
                        tag |= 8;
                        return tag ^ (v as u32) ^ 0x8000_0000;
                    }
                    v = mix(v, p4);
                    let mut i5 = 0;
                    while i5 < n5 {
                        let p5 = probe(5, v, i5);
                        if p5 & 15 == 3 {
                            tag |= 16;
                            break;
                        }
                        v = mix(v, p5) ^ i5 as u64;
                        i5 += 1;
                    }
                    i4 += 1;
                }
                i3 += 1;
            }
            i2 += 1;
        }
        i1 += 1;
    }
    (v as u32) ^ ((v >> 32) as u32) ^ (tag << 24) ^ i1
}
