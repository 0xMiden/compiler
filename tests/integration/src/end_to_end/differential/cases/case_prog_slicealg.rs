// A reorder buffer over a `[u32; 64]` stack array (campaign 27, program 5):
// the in-place slice algorithms a user reaches for — `rotate_left` /
// `rotate_right` by a runtime count, `reverse`, `fill`, `swap`,
// `split_at_mut`, `copy_from_slice`, `copy_within` between distinct ranges,
// `chunks` / `chunks_exact_mut` — followed by a hand-written insertion sort
// and by `sort_unstable` / `sort_unstable_by_key` (which DO link: with
// `-Zbuild-std-features=optimize_for_size` core's unstable sort is the
// non-recursive heapsort), the two results cross-checked against each other,
// then `binary_search` on the sorted half and a sortedness scan.

fn seed(input1: u32, input2: u32) -> [u32; 64] {
    core::array::from_fn(|i| {
        let k = i as u32;
        (input1.wrapping_mul(k.wrapping_add(3)) ^ input2.rotate_right(k & 31)).wrapping_add(k)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = seed(input1, input2);
    let k = (input2 % 64) as usize;

    a.rotate_left(k);
    a[..32].rotate_right(k % 32);
    a[32..].reverse();
    a.swap(k, 63 - k);
    {
        let (lo, hi) = a.split_at_mut(32);
        hi[..16].copy_from_slice(&lo[16..]);
        lo[..8].fill(input1 ^ 0x5a5a_5a5a);
    }
    // Distinct source and destination ranges (`src == dst` traps in MASM —
    // the known `copy_same_pos` class — so the offset is forced non-zero).
    let src = k % 24;
    a.copy_within(src..src + 8, src + 24);
    for c in a.chunks_exact_mut(8) {
        c[0] = c[0].wrapping_add(c[7]);
    }

    let mut by_hand = [0u32; 32];
    by_hand.copy_from_slice(&a[..32]);
    let mut i = 1usize;
    while i < 32 {
        let v = by_hand[i];
        let mut j = i;
        while j > 0 && by_hand[j - 1] > v {
            by_hand[j] = by_hand[j - 1];
            j -= 1;
        }
        by_hand[j] = v;
        i += 1;
    }

    let mut by_core = [0u32; 32];
    by_core.copy_from_slice(&a[..32]);
    by_core[..(8 + k % 25)].sort_unstable();
    by_core[(8 + k % 25)..].sort_unstable_by_key(|v| !*v);

    // Element-wise agreement check: slice `==` is not linkable in a guest.
    let agree = by_hand.iter().zip(by_core.iter()).filter(|(x, y)| x == y).count() as u32;

    let probe = a[k % 32];
    let found = match by_hand.binary_search(&probe) {
        Ok(idx) => idx as u32,
        Err(idx) => 0x100 + idx as u32,
    };
    let sorted = by_hand.windows(2).all(|w| w[0] <= w[1]) as u32;
    let present = by_hand.contains(&probe) as u32;

    let mut h = 2166136261u32;
    for chunk in a.chunks(5) {
        h = h.rotate_left(7) ^ chunk.iter().fold(0u32, |x, &v| x.wrapping_add(v));
    }
    for &v in by_hand.iter() {
        h = (h ^ v).wrapping_mul(16777619);
    }
    for &v in by_core.iter() {
        h = h.wrapping_mul(31).wrapping_add(v);
    }
    h.wrapping_add(found)
        .wrapping_add(sorted * 4096)
        .wrapping_add(present * 17)
        .wrapping_add(agree * 3)
}
