// Trap parity: nested indexing into a `[[u16; 4]; 4]`. The row comes from
// `input1 % 5` and the column from `input2 % 5`, so each dimension has one
// out-of-range value (4) reachable independently: the outer index panics
// before the inner one, and either must trap on both targets.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let grid: [[u16; 4]; 4] = [
        [1, 2, 3, 4],
        [5, 6, 7, 8],
        [9, 10, 11, 12],
        [13, 14, 15, 16],
    ];
    let r = (input1 % 5) as usize;
    let c = (input2 % 5) as usize;
    let cell = grid[r][c] as u32;
    // A second, transposed read so both dimensions are indexed by both
    // inputs; it is reached only when the first read stayed in range.
    let swapped = grid[c][r] as u32;
    cell.wrapping_mul(1000).wrapping_add(swapped) ^ ((r as u32) << 8) ^ (c as u32)
}
