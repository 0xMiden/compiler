// One local ASSIGNED in each of eight `match` arms and read after the match,
// beside a second local that each arm only reads. The assigned local has one
// `hir.store_local` per arm, so Local2Reg's "stored more than once" guard
// rejects it at every debug level; the arm-local temporaries are the slots
// promotion can take. The eight arms lower to a dense `br_table`, so the
// value the join reads has to survive the dispatch lowering unchanged whether
// or not the stores were erased.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let base = input2 | 1;
    let mut picked: u32 = 0;
    let mut hits: u32 = 0;
    match input1 & 7 {
        0 => {
            picked = base.rotate_left(1);
            hits = hits.wrapping_add(1);
        }
        1 => {
            picked = base ^ 0xa5a5_a5a5;
            hits = hits.wrapping_add(2);
        }
        2 => {
            picked = base.wrapping_mul(0x0101_0101);
            hits = hits.wrapping_add(3);
        }
        3 => {
            picked = base.rotate_right(5) ^ input1;
            hits = hits.wrapping_add(4);
        }
        4 => {
            picked = base.wrapping_sub(input1 >> 3);
            hits = hits.wrapping_add(5);
        }
        5 => {
            picked = (base >> 7) | (base << 9);
            hits = hits.wrapping_add(6);
        }
        6 => {
            picked = base ^ (input1 & 0x00ff_ff00);
            hits = hits.wrapping_add(7);
        }
        _ => {
            picked = base.wrapping_add(input1.rotate_left(13));
            hits = hits.wrapping_add(8);
        }
    }
    // Both the multi-stored local and the counter are read after the join.
    picked.rotate_left(hits & 31) ^ hits.wrapping_mul(0x9e37_79b9)
}
