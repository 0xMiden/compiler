// Switch-lowering shapes. A four-exit loop produces a contiguous exit
// discriminator (scf.index_switch dispatch with results), and two
// equality-comparison chains over a hashed value are re-fused by the
// `SimplifyCondBrLikeSwitch` canonicalization into cf.switch ops: one with
// contiguous non-zero cases {7,8,9} (binary-search lowering behind an
// interval guard) and one with sparse cases {21,77,200} (linear search).
// LLVM keeps the chains as br_ifs since they are too sparse for br_table.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut x = input1 | 1;
    let mut i = 0u32;
    let n = input2 % 173;
    let tag = loop {
        if i >= n {
            break 0u32;
        }
        x = x.wrapping_mul(0x0808_8405).wrapping_add(i);
        if x & 0xf000_0000 == 0x9000_0000 {
            break 1;
        }
        if x & 0x0f00_0000 == 0x0600_0000 {
            break 2;
        }
        if x & 0x00f0_0000 == 0x0030_0000 {
            break 3;
        }
        i = i.wrapping_add(1);
    };

    let h = x.wrapping_mul(0x9e37_79b9) % 251;
    // Contiguous non-zero constants — fused into a switch over {7,8,9}.
    let a = if h == 7 {
        x ^ 0x1111
    } else if h == 8 {
        x.rotate_left(3).wrapping_add(i)
    } else if h == 9 {
        x.wrapping_sub(i).rotate_right(2)
    } else {
        x >> 3
    };
    // Sparse constants — fused into a switch over {21,77,200}.
    let b = if h == 21 {
        a.wrapping_add(0x2222)
    } else if h == 77 {
        a ^ tag
    } else if h == 200 {
        a.rotate_left(9)
    } else {
        a.wrapping_mul(5)
    };
    b ^ tag.wrapping_mul(0x0101_0101)
}
