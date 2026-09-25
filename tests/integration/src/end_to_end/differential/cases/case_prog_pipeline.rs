// A sample-stream reducer written as an iterator pipeline (campaign 27,
// program 4): a 48-entry sample array derived from the inputs is framed with
// `chunks_exact`, differenced with `windows`, gated with `filter_map`,
// decimated with `step_by`, run-length-collapsed with `peekable`,
// accumulated with `scan` / `fold`, cut with `take_while` / `skip_while`,
// widened with `flat_map`, and paired with `zip` / `rev` / `enumerate` — the
// adapters a user reaches for, composed the way a signal path composes them.

fn samples(input1: u32, input2: u32) -> [u32; 48] {
    core::array::from_fn(|i| {
        let k = i as u32;
        input1
            .rotate_left(k & 31)
            .wrapping_add(input2.wrapping_mul(k | 1))
            ^ (k << 3)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = samples(input1, input2);
    let live = &s[..(8 + (input2 % 41) as usize)];
    let threshold = input1 | 1;

    // Frames of four: fold each frame to one value, then reduce the frames.
    let framed = live
        .chunks_exact(4)
        .map(|c| c.iter().fold(0u32, |h, &v| h.rotate_left(3) ^ v))
        .fold(0x811c_9dc5u32, |h, v| h.wrapping_mul(31).wrapping_add(v));

    // Moving difference over a three-sample window, decimated.
    let moving = live
        .windows(3)
        .step_by(2)
        .map(|w| w[0].wrapping_sub(w[2]) >> 1)
        .fold(0u32, |a, v| a.wrapping_add(v));

    // Gate, then accumulate with a running state.
    let gated = live
        .iter()
        .filter_map(|&v| (v % 7 != 0).then_some(v & 0x00ff_ffff))
        .scan(1u32, |st, v| {
            *st = st.wrapping_mul(3).wrapping_add(v);
            Some(*st >> 2)
        })
        .fold(0u32, |a, v| a ^ v);

    // Prefix and suffix cuts.
    let head = live.iter().take_while(|&&v| v < threshold).count() as u32;
    let tail = live.iter().skip_while(|&&v| v >= threshold).count() as u32;

    // Run-length collapse of the low nibble.
    let mut runs = 0u32;
    let mut longest = 0u32;
    let mut it = live.iter().map(|&v| v & 0xf).peekable();
    while let Some(v) = it.next() {
        let mut run = 1u32;
        while it.peek() == Some(&v) {
            it.next();
            run += 1;
        }
        runs += 1;
        if run > longest {
            longest = run;
        }
    }

    // Byte fan-out and a mirrored pairing.
    let widened = live
        .iter()
        .flat_map(|&v| [v as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8])
        .enumerate()
        .fold(7u32, |h, (i, byte)| h.rotate_left(5) ^ (byte as u32).wrapping_mul(i as u32 + 1));
    let mirrored = live
        .iter()
        .rev()
        .zip(live.iter())
        .map(|(&x, &y)| x.wrapping_sub(y))
        .fold(0u32, |a, v| a.wrapping_add(v));

    let extremes = live.iter().max().copied().unwrap_or(0)
        ^ live.iter().min().copied().unwrap_or(0)
        ^ live.iter().position(|&v| v > threshold).unwrap_or(63) as u32
        ^ live.iter().rposition(|&v| v & 1 == 0).unwrap_or(62) as u32;

    let seeded = core::iter::once(input1)
        .chain(core::iter::repeat(input2 | 3).take(3))
        .chain(live.iter().copied().take(5))
        .fold(0u32, |h, v| h.wrapping_mul(17).wrapping_add(v));
    let halving =
        core::iter::successors(Some(threshold), |&v| (v > 5).then(|| v >> 2)).count() as u32;

    framed
        .wrapping_add(moving)
        .wrapping_add(gated)
        .wrapping_add(head.wrapping_mul(101))
        .wrapping_add(tail.wrapping_mul(7))
        .wrapping_add(runs.wrapping_mul(13))
        .wrapping_add(longest)
        .wrapping_add(widened)
        .wrapping_add(mirrored)
        .wrapping_add(extremes)
        .wrapping_add(seeded)
        .wrapping_add(halving)
}
