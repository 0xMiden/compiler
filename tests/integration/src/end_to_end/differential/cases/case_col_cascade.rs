// Eight sequential 8-trip loops whose only in-body branch is an EMPTY
// `continue` arm, chained through an accumulator over a filled array. Each
// such loop drives one full column-removal cascade: cfg-to-scf gives the loop
// an exit-dispatch payload column, `WhileUnusedResult` drops the loop result,
// the yield operand dies and `IndexSwitchRemoveUnusedResults` then removes the
// switch column and remaps the survivors. Emptiness of the `continue` arm is
// what makes the cascade happen -- an arm that updates any carried variable
// produces no unused column at all.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut arr = [0u32; 4];
    let mut i = 0;
    while i < 4 {
        arr[i] = input1.wrapping_mul(i as u32 + 1) ^ input2.rotate_left(i as u32);
        i += 1;
    }
    let mut acc: u32 = input2 | 1;
    let mut s0 = acc;
    let mut t0 = 0;
    while t0 < 8 {
        t0 += 1;
        if (input2 >> (t0 + 0)) & 1 == 0 {
            continue;
        }
        s0 = s0.wrapping_add(arr[t0 & 3].rotate_right(t0 as u32));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s0;
    let mut s1 = acc;
    let mut t1 = 0;
    while t1 < 8 {
        t1 += 1;
        if (input2 >> (t1 + 1)) & 1 == 0 {
            continue;
        }
        s1 = s1.wrapping_add(arr[t1 & 3].rotate_right(t1 as u32));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s1;
    let mut s2 = acc;
    let mut t2 = 0;
    while t2 < 8 {
        t2 += 1;
        if (input2 >> (t2 + 2)) & 1 == 0 {
            continue;
        }
        s2 = s2.wrapping_add(arr[t2 & 3].rotate_right(t2 as u32));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s2;
    let mut s3 = acc;
    let mut t3 = 0;
    while t3 < 8 {
        t3 += 1;
        if (input2 >> (t3 + 3)) & 1 == 0 {
            continue;
        }
        s3 = s3.wrapping_add(arr[t3 & 3].rotate_right(t3 as u32));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s3;
    let mut s4 = acc;
    let mut t4 = 0;
    while t4 < 8 {
        t4 += 1;
        if (input2 >> (t4 + 4)) & 1 == 0 {
            continue;
        }
        s4 = s4.wrapping_add(arr[t4 & 3].rotate_right(t4 as u32));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s4;
    let mut s5 = acc;
    let mut t5 = 0;
    while t5 < 8 {
        t5 += 1;
        if (input2 >> (t5 + 5)) & 1 == 0 {
            continue;
        }
        s5 = s5.wrapping_add(arr[t5 & 3].rotate_right(t5 as u32));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s5;
    let mut s6 = acc;
    let mut t6 = 0;
    while t6 < 8 {
        t6 += 1;
        if (input2 >> (t6 + 6)) & 1 == 0 {
            continue;
        }
        s6 = s6.wrapping_add(arr[t6 & 3].rotate_right(t6 as u32));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s6;
    let mut s7 = acc;
    let mut t7 = 0;
    while t7 < 8 {
        t7 += 1;
        if (input2 >> (t7 + 7)) & 1 == 0 {
            continue;
        }
        s7 = s7.wrapping_add(arr[t7 & 3].rotate_right(t7 as u32));
    }
    acc = acc.wrapping_mul(0x0100_0193) ^ s7;
    acc ^ arr[1]
}
