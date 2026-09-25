// Sorting and searching (campaign 17, program 7): a `[u32; 64]` stack
// array filled by an xorshift generator seeded from the inputs is sorted
// three ways — insertion sort, heap sort (sift-down with runtime heap
// bounds) and a bottom-up merge sort alternating between the array and a
// scratch buffer — then checked for sortedness and pairwise equality,
// binary-searched for a present key and a probably-absent one, and ranked
// (count below the first input); the sorted array, the search results,
// the ranks and every check flag are folded into the result.
fn fill(arr: &mut [u32; 64], seed: u32) {
    let mut x = seed | 1;
    let mut i = 0usize;
    while i < 64 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        // Duplicates on purpose (low bits masked on some entries).
        arr[i] = if i & 7 == 3 { x & 0xffff_0000 } else { x };
        i += 1;
    }
}

fn insertion_sort(a: &mut [u32; 64]) {
    let mut i = 1usize;
    while i < 64 {
        let key = a[i];
        let mut j = i;
        while j > 0 && a[j - 1] > key {
            a[j] = a[j - 1];
            j -= 1;
        }
        a[j] = key;
        i += 1;
    }
}

fn sift_down(a: &mut [u32; 64], start: usize, end: usize) {
    let mut root = start;
    loop {
        let mut child = 2 * root + 1;
        if child >= end {
            break;
        }
        if child + 1 < end && a[child] < a[child + 1] {
            child += 1;
        }
        if a[root] < a[child] {
            a.swap(root, child);
            root = child;
        } else {
            break;
        }
    }
}

fn heap_sort(a: &mut [u32; 64]) {
    let mut start = 32usize;
    while start > 0 {
        start -= 1;
        sift_down(a, start, 64);
    }
    let mut end = 64usize;
    while end > 1 {
        end -= 1;
        a.swap(0, end);
        sift_down(a, 0, end);
    }
}

fn merge_sort(a: &mut [u32; 64]) {
    let mut tmp = [0u32; 64];
    let mut width = 1usize;
    let mut in_tmp = false;
    while width < 64 {
        let mut lo = 0usize;
        while lo < 64 {
            let mid = lo + width;
            let hi = if lo + 2 * width < 64 {
                lo + 2 * width
            } else {
                64
            };
            let (mut i, mut j, mut k) = (lo, mid, lo);
            while k < hi {
                let take_left = j >= hi
                    || (i < mid
                        && (if in_tmp {
                            tmp[i] <= tmp[j]
                        } else {
                            a[i] <= a[j]
                        }));
                let v = if in_tmp {
                    if take_left {
                        let v = tmp[i];
                        i += 1;
                        v
                    } else {
                        let v = tmp[j];
                        j += 1;
                        v
                    }
                } else if take_left {
                    let v = a[i];
                    i += 1;
                    v
                } else {
                    let v = a[j];
                    j += 1;
                    v
                };
                if in_tmp {
                    a[k] = v;
                } else {
                    tmp[k] = v;
                }
                k += 1;
            }
            lo += 2 * width;
        }
        in_tmp = !in_tmp;
        width *= 2;
    }
    if in_tmp {
        a.copy_from_slice(&tmp);
    }
}

// Element-wise equality (array `==` would lower to a `memcmp` libcall that
// the guest link does not provide).
fn same64(a: &[u32; 64], b: &[u32; 64]) -> bool {
    let mut i = 0usize;
    while i < 64 {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn is_sorted(a: &[u32; 64]) -> bool {
    let mut i = 1usize;
    while i < 64 {
        if a[i - 1] > a[i] {
            return false;
        }
        i += 1;
    }
    true
}

// Lower-bound binary search: (found, insertion index).
fn bsearch(a: &[u32; 64], key: u32) -> (bool, usize) {
    let mut lo = 0usize;
    let mut hi = 64usize;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if a[mid] < key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    (lo < 64 && a[lo] == key, lo)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut base = [0u32; 64];
    fill(&mut base, input1 ^ input2.rotate_left(16) ^ 0x2545_f491);
    let mut a1 = base;
    let mut a2 = base;
    let mut a3 = base;
    insertion_sort(&mut a1);
    heap_sort(&mut a2);
    merge_sort(&mut a3);
    let ok = (is_sorted(&a1) as u32) | (is_sorted(&a2) as u32) << 1 | (is_sorted(&a3) as u32) << 2;
    let same = (same64(&a1, &a2) as u32) << 3 | (same64(&a2, &a3) as u32) << 4;
    let present = base[(input2 % 64) as usize];
    let (f1, i1) = bsearch(&a1, present);
    let (f2, i2) = bsearch(&a2, present ^ 1);
    let (f3, i3) = bsearch(&a3, input1);
    // Rank of input1 by a linear scan, cross-checked against the search.
    let mut rank = 0u32;
    let mut i = 0usize;
    while i < 64 {
        rank += (base[i] < input1) as u32;
        i += 1;
    }
    let rank_ok = ((rank as usize == i3) as u32) << 5;
    let flags = ok | same | rank_ok | (f1 as u32) << 6 | (f2 as u32) << 7 | (f3 as u32) << 8;
    let mut acc =
        flags.wrapping_mul(0x9e37_79b9) ^ (i1 as u32) << 8 ^ (i2 as u32) << 16 ^ (i3 as u32) << 24;
    i = 0;
    while i < 64 {
        acc = acc.rotate_left(5) ^ a1[i].wrapping_add(i as u32);
        i += 1;
    }
    acc ^ rank
}
