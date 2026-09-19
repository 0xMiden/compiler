// `core`'s unstable sorts in a guest (campaign 27, Part A): `sort_unstable`,
// `sort_unstable_by_key` and `sort_unstable_by` over runtime-length
// sub-slices of a `[u32; 64]`, plus `binary_search` / `binary_search_by_key`
// on the result and an `is_sorted` check. Campaign 17 recorded the unstable
// sorts as unusable ("found a cycle in the call graph", ipnsort's recursive
// `quicksort` / `median_of_medians`); the three SORTS link and assemble
// today because the compiler builds `core` with
// `-Zbuild-std-features=optimize_for_size`, whose unstable sort is the
// NON-recursive `core::slice::sort::unstable::heapsort` (verified in the
// guest wasm's name section). `select_nth_unstable` still does not — it
// keeps the recursive `median_of_medians` (see the
// `core_select_nth_nolink` case). The insertion sort beside them is the
// reference order the sorted output is checked against.

fn seed(input1: u32, input2: u32) -> [u32; 64] {
    core::array::from_fn(|i| {
        let k = i as u32;
        input1.rotate_left(k & 31) ^ input2.wrapping_mul(k + 1)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let base = seed(input1, input2);
    let n = 4 + (input2 % 60) as usize;

    let mut a = base;
    a[..n].sort_unstable();

    let mut b = base;
    b[..n].sort_unstable_by_key(|v| (*v & 0xffff, *v >> 16));

    let mut c = base;
    c[..n].sort_unstable_by(|x, y| y.cmp(x));

    // Reference order for the ascending sort.
    let mut r = base;
    let mut i = 1usize;
    while i < n {
        let v = r[i];
        let mut j = i;
        while j > 0 && r[j - 1] > v {
            r[j] = r[j - 1];
            j -= 1;
        }
        r[j] = v;
        i += 1;
    }
    let agree = a[..n].iter().zip(r[..n].iter()).filter(|(x, y)| x == y).count() as u32;

    let probe = base[(input2 % 64) as usize];
    let found = match a[..n].binary_search(&probe) {
        Ok(idx) => idx as u32,
        Err(idx) => 0x100 + idx as u32,
    };
    let by_key = match b[..n].binary_search_by_key(&(probe & 0xffff), |v| *v & 0xffff) {
        Ok(idx) => idx as u32,
        Err(idx) => 0x200 + idx as u32,
    };

    let mut h = 2166136261u32;
    let mut m = 0usize;
    while m < n {
        h = (h ^ a[m]).wrapping_mul(16777619);
        h = h.rotate_left(3) ^ b[m];
        h = h.wrapping_mul(31).wrapping_add(c[m]);
        m += 1;
    }
    h.wrapping_add(agree * 7)
        .wrapping_add(found)
        .wrapping_add(by_key)
        .wrapping_add(a[..n].is_sorted() as u32 * 4096)
        .wrapping_add(c[..n].is_sorted_by(|x, y| x >= y) as u32 * 8192)
}
