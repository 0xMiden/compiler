// A priority table ordered by derived `Ord` (campaign 27, program 9):
// `array::from_fn` builds twelve composite keys (a struct whose fields are a
// `u16`, an enum with a payload variant, a tuple and a `bool`), `array::map`
// projects them, and the ordering APIs a user reaches for do the work —
// `max_by_key`, `min_by`, `clamp`, `cmp`/`partial_cmp`, `Ordering::then_with`
// and `is_sorted_by`, with an insertion sort by the derived `Ord` as the
// reference order (`sort_unstable_by` is used on a projection so the two
// orders can be cross-checked element-wise; slice `==` is not linkable).

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
struct Key {
    class: Class,
    weight: u16,
    span: (u8, i8),
    urgent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
enum Class {
    #[default]
    Idle,
    Batch(u8),
    Interactive {
        nice: i8,
    },
}

fn key(i: u32, input1: u32, input2: u32) -> Key {
    let v = input1.rotate_left(i * 5) ^ input2.wrapping_mul(i + 1);
    Key {
        class: match v % 3 {
            0 => Class::Idle,
            1 => Class::Batch(v as u8),
            _ => Class::Interactive { nice: v as i8 },
        },
        weight: (v >> 8) as u16,
        span: ((v >> 3) as u8, v as i8),
        urgent: v & 0x8000_0000 != 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let keys: [Key; 12] = core::array::from_fn(|i| key(i as u32, input1, input2));
    let weights: [u16; 12] = keys.map(|k| k.weight);
    let pairs: [(u16, i8); 12] = keys.map(|k| (k.weight, k.span.1));

    let mut acc = 0u32;
    acc = acc.wrapping_add(keys.iter().max().map_or(0, |k| k.weight as u32));
    acc = acc.wrapping_add(keys.iter().min().map_or(0, |k| k.weight as u32) << 4);
    acc = acc.wrapping_add(keys.iter().max_by_key(|k| (k.urgent, k.weight)).map_or(0, |k| {
        k.span.0 as u32
    }));
    acc = acc.wrapping_add(
        keys.iter()
            .min_by(|a, b| a.span.1.cmp(&b.span.1).then_with(|| b.weight.cmp(&a.weight)))
            .map_or(0, |k| k.weight as u32),
    );
    acc = acc.wrapping_add(pairs.iter().max().map_or(0, |p| p.0 as u32 ^ p.1 as u32));
    acc = acc.wrapping_add(weights.iter().copied().max().unwrap_or(0) as u32);

    // Reference order: insertion sort by the derived `Ord`.
    let mut sorted = keys;
    let mut i = 1usize;
    while i < 12 {
        let v = sorted[i];
        let mut j = i;
        while j > 0 && sorted[j - 1] > v {
            sorted[j] = sorted[j - 1];
            j -= 1;
        }
        sorted[j] = v;
        i += 1;
    }
    // The same order via core's unstable sort on a projection.
    let mut projected: [(Class, u16); 12] = keys.map(|k| (k.class, k.weight));
    projected.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let agree = sorted
        .iter()
        .zip(projected.iter())
        .filter(|(k, p)| k.class == p.0 && k.weight == p.1)
        .count() as u32;
    let ordered = sorted.is_sorted() as u32 + weights.is_sorted_by(|a, b| a <= b) as u32 * 2;

    let lo = keys[(input1 % 12) as usize];
    let hi = keys[(input2 % 12) as usize];
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let clamped = sorted[(input1 % 12) as usize].clamp(lo, hi);
    acc = acc.wrapping_add(clamped.weight as u32);
    acc = acc.wrapping_add(core::cmp::max(lo, hi).weight as u32);
    acc = acc.wrapping_add(core::cmp::min(lo.span.0, hi.span.0) as u32);
    acc = acc.wrapping_add(match lo.partial_cmp(&hi) {
        Some(core::cmp::Ordering::Less) => 1,
        Some(core::cmp::Ordering::Equal) => 2,
        Some(core::cmp::Ordering::Greater) => 3,
        None => 4,
    });
    acc = acc.wrapping_add((input1 as i32).clamp(-1000, 1000) as u32);
    acc = acc.wrapping_add(input2.clamp(17, 4096));

    let mut k = 0usize;
    while k < 12 {
        let e = sorted[k];
        acc = acc.wrapping_mul(31).wrapping_add(e.weight as u32);
        acc = acc.wrapping_add(match e.class {
            Class::Idle => 1,
            Class::Batch(b) => b as u32,
            Class::Interactive { nice } => (nice as i32 as u32) ^ 0xff,
        });
        acc = acc.wrapping_add(e.span.0 as u32 ^ (e.span.1 as i32 as u32));
        acc = acc.wrapping_add(e.urgent as u32);
        k += 1;
    }
    acc.wrapping_add(agree * 7).wrapping_add(ordered * 1024)
}
