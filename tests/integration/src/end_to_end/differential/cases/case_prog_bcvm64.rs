// Bytecode interpreter with 64-bit bookkeeping (campaign 21, cliff shape:
// in-loop wide dispatch with u64 state).  A 20-opcode stack machine runs one
// of three `.rodata` program images; the fetch loop dispatches on the opcode
// with a dense `match` (four opcodes share the default body and the opcodes
// 20..31 are invalid, which the images never contain), and every arm updates
// the same six u64 bookkeeping words — a state hash, a checksum, a gas
// counter, a flag mask, a high-water mark and a trace fold — which are
// combined in ONE commitment expression at the bottom of the loop and folded
// into the result after it.  The six rotate constants 7, 13, 19, 29, 37 and 43
// are shared by the seeding code, the dispatch arms and the final fold.
const C0: u32 = 7;
const C1: u32 = 13;
const C2: u32 = 19;
const C3: u32 = 29;
const C4: u32 = 37;
const C5: u32 = 43;

const GOLD: u64 = 0x9e37_79b9_7f4a_7c15;

// op = (opcode << 3) | small immediate.
const PROGS: [[u8; 24]; 3] = [
    [
        0x01, 0x0a, 0x11, 0x22, 0x33, 0x09, 0x41, 0x52, 0x63, 0x19, 0x71, 0x82, 0x93, 0x29, 0x0b,
        0x12, 0x23, 0x34, 0x45, 0x56, 0x67, 0x78, 0x89, 0x00,
    ],
    [
        0x0a, 0x51, 0x62, 0x13, 0x24, 0x35, 0x46, 0x57, 0x68, 0x79, 0x8a, 0x9b, 0x0c, 0x1d, 0x2e,
        0x3f, 0x40, 0x71, 0x82, 0x93, 0x14, 0x25, 0x36, 0x00,
    ],
    [
        0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0x09, 0x1a, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f,
        0x70, 0x81, 0x92, 0x03, 0x14, 0x25, 0x36, 0x47, 0x00,
    ],
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let image = &PROGS[(input1 % 3) as usize];

    let mut stack = [0u64; 16];
    let mut sp = 0usize;
    stack[sp] = (input1 as u64).rotate_left(C0) | 1;
    sp += 1;
    stack[sp] = (input2 as u64).rotate_left(C1) | 2;
    sp += 1;

    // Bookkeeping words, all live across the dispatch.
    let mut hash = GOLD.rotate_left(C2) ^ (input1 as u64);
    let mut check = GOLD.rotate_left(C3) ^ (input2 as u64);
    let mut gas = 512u64;
    let mut flags = 0u64;
    let mut high = 0u64;
    let mut trace = GOLD.rotate_left(C4);

    let mut pc = 0usize;
    let mut steps = 0u32;
    let budget = 24 + (input2 % 40);
    while steps < budget && pc < 24 {
        let word = image[pc];
        pc += 1;
        let opcode = ((word >> 3) as u32).wrapping_add(input2 >> 28) % 32;
        let imm = (word & 7) as u64;
        match opcode {
            0 => {
                // push immediate
                if sp < 16 {
                    stack[sp] = imm.wrapping_mul(GOLD) ^ hash.rotate_left(C0);
                    sp += 1;
                }
                gas = gas.wrapping_sub(1);
            }
            1 => {
                // add
                if sp >= 2 {
                    let b = stack[sp - 1];
                    let a = stack[sp - 2];
                    stack[sp - 2] = a.wrapping_add(b);
                    sp -= 1;
                }
                check ^= check.rotate_left(C1);
            }
            2 => {
                // mul
                if sp >= 2 {
                    let b = stack[sp - 1];
                    let a = stack[sp - 2];
                    stack[sp - 2] = a.wrapping_mul(b | 1);
                    sp -= 1;
                }
                gas = gas.wrapping_sub(3);
            }
            3 => {
                // xor-rotate
                if sp >= 1 {
                    stack[sp - 1] ^= stack[sp - 1].rotate_left(C2);
                }
                flags |= 1 << 3;
            }
            4 => {
                // dup
                if sp >= 1 && sp < 16 {
                    stack[sp] = stack[sp - 1];
                    sp += 1;
                }
                trace = trace.wrapping_add(trace.rotate_left(C3));
            }
            5 => {
                // swap
                if sp >= 2 {
                    let t = stack[sp - 1];
                    stack[sp - 1] = stack[sp - 2];
                    stack[sp - 2] = t;
                }
                flags |= 1 << 5;
            }
            6 => {
                // shift by immediate
                if sp >= 1 {
                    stack[sp - 1] = stack[sp - 1].rotate_left((imm as u32) + C0);
                }
                hash ^= hash.rotate_left(C1);
            }
            7 => {
                // conditional skip
                if sp >= 1 && stack[sp - 1] & 1 == 0 {
                    pc += 1;
                }
                gas = gas.wrapping_sub(2);
            }
            8 => {
                // load from the low stack slot
                let idx = (imm as usize) & 7;
                let v = stack[idx];
                if sp < 16 {
                    stack[sp] = v ^ high.rotate_left(C4);
                    sp += 1;
                }
            }
            9 => {
                // store into the low stack slot
                if sp >= 1 {
                    let idx = (imm as usize) & 7;
                    stack[idx] = stack[sp - 1];
                    sp -= 1;
                }
                check = check.wrapping_mul(GOLD);
            }
            10 => {
                // fold the top into the hash
                if sp >= 1 {
                    hash = (hash ^ stack[sp - 1]).wrapping_mul(GOLD).rotate_left(C5);
                }
            }
            11 => {
                // backward jump, bounded by the step budget
                if pc > 4 && gas > 64 {
                    pc -= 4;
                }
                gas = gas.wrapping_sub(5);
            }
            12 => {
                // high-water mark
                if sp >= 1 && stack[sp - 1] > high {
                    high = stack[sp - 1];
                }
            }
            13 => {
                // pop
                if sp >= 1 {
                    sp -= 1;
                }
                flags |= 1 << 13;
            }
            14 => {
                // negate
                if sp >= 1 {
                    stack[sp - 1] = stack[sp - 1].wrapping_neg();
                }
                trace ^= trace.rotate_left(C5);
            }
            15 => {
                // compare
                if sp >= 2 {
                    let b = stack[sp - 1];
                    let a = stack[sp - 2];
                    stack[sp - 2] = if a < b { 1 } else { 0 };
                    sp -= 1;
                }
                flags |= 1 << 15;
            }
            16 | 17 | 18 | 19 => {
                // reserved-but-defined opcodes: the shared fallback body
                hash = hash.rotate_left(C0) ^ imm;
                check = check.wrapping_add(imm.rotate_left(C2));
            }
            _ => {
                // Invalid opcode: the images contain none, but the decoder
                // cannot know that.
                flags |= 1 << 31;
                gas = gas.wrapping_sub(7);
            }
        }

        // Per-step state commitment: every bookkeeping word is live here.
        let commit = (hash ^ check.rotate_left(C0))
            .wrapping_add(gas ^ flags.rotate_left(C1))
            .wrapping_mul(trace | 1)
            ^ high.rotate_left(C2);
        hash = hash.wrapping_add(commit.rotate_left(C3));
        trace ^= commit.rotate_left(C4);
        if gas < 16 {
            break;
        }
        steps += 1;
    }

    let mut out = hash.rotate_left(C0) ^ check.rotate_left(C1);
    out = out.wrapping_add(gas.rotate_left(C2));
    out ^= flags.rotate_left(C3);
    out = out.wrapping_sub(high.rotate_left(C4));
    out ^= trace.rotate_left(C5);
    out ^= (sp as u64).wrapping_mul(GOLD) ^ (steps as u64);
    (out as u32) ^ ((out >> 32) as u32)
}
