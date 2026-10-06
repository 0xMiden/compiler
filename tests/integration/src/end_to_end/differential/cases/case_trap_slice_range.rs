// Trap parity: slice range indexing. `&data[a..b]` over a `[u32; 10]` with
// both endpoints in 0..12 panics in two distinct ways — `slice index starts
// at N but ends at M` when `a > b`, and `range end index M out of range`
// when `b > 10` — and the open-ended forms `&data[a..]` / `&data[..b]` share
// the second. Both targets must trap on exactly those rows and agree on the
// folded slice everywhere else, including the empty slice at `a == b`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let data: [u32; 10] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29];
    let a = (input1 % 13) as usize;
    let b = (input2 % 13) as usize;
    let mut acc = 0u32;
    for &v in &data[a..b] {
        acc = acc.wrapping_mul(31).wrapping_add(v);
    }
    let tail = data[a..].len() as u32;
    let head = data[..b].len() as u32;
    acc ^ (tail << 8) ^ head
}
