// Bytecode interpreter (campaign 17, program 4): an 18-opcode stack VM
// (push / dup / swap / over / add / mul / xor / rotl / shr / sub / dec /
// jmp / jz / call / ret / load / store / halt) executing one of four
// 64-byte program images from a `.rodata` table selected by the low input
// bits, with the inputs seeding the operand stack and the VM memory (loop
// counters, multipliers, an index), a bounded step budget, a return-address
// stack and fault codes for stack / call-stack misuse. The dispatch is a
// dense `match` (one `br_table`) inside the fetch loop; the operand stack,
// memory, return stack, registers and the fault / step counters are all
// folded into the result.
static PROGS: [[u8; 64]; 4] = [
    // hash_loop: multiply-xor-rotate over mem[1], mem[2], counted by mem[0].
    [
        1, 0, 13, 10, 48, 1, 0, 13, 16, 1, 0, 14, 1, 1, 13, 1, 2, 13, 5, 1, 1, 13, 6, 1, 7, 7, 1,
        1, 14, 1, 2, 13, 1, 1, 13, 4, 1, 3, 8, 1, 2, 13, 6, 1, 2, 14, 9, 0, 1, 1, 13, 1, 2, 13, 6,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    // calls: subroutine calls in a countdown loop; F mixes mem[1] with mem[3].
    [
        1, 0, 13, 2, 10, 11, 11, 14, 16, 9, 3, 11, 30, 0, 1, 1, 13, 2, 1, 5, 7, 6, 1, 3, 13, 4, 1,
        1, 14, 12, 1, 3, 13, 1, 1, 13, 15, 1, 3, 14, 1, 1, 13, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    // fib_stack: Fibonacci-style SWAP / OVER / ADD shuffling, counted by mem[0].
    [
        1, 1, 13, 1, 2, 13, 1, 0, 13, 10, 23, 1, 0, 13, 16, 1, 0, 14, 3, 17, 4, 9, 6, 17, 6, 1, 1,
        14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0,
    ],
    // mem_walk: indexed fill of mem[4..] then a xor-fold walk down, index in mem[3].
    [
        1, 3, 13, 1, 15, 15, 10, 33, 1, 3, 13, 1, 1, 13, 5, 1, 3, 13, 1, 4, 4, 14, 1, 3, 13, 1, 1,
        4, 1, 3, 14, 9, 0, 1, 0, 1, 3, 13, 10, 57, 1, 3, 13, 13, 6, 1, 13, 7, 1, 3, 13, 16, 1, 3,
        14, 9, 35, 1, 2, 14, 0, 0, 0, 0,
    ],
];

const STEP_BUDGET: u32 = 600;

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let prog = &PROGS[(input1 & 3) as usize];
    let mut stack = [0u32; 16];
    let mut mem = [0u32; 16];
    let mut cstack = [0u8; 8];
    stack[0] = input1;
    stack[1] = input2;
    let mut sp = 2usize;
    let mut csp = 0usize;
    mem[0] = (input1 >> 2) % 40;
    mem[1] = input2 | 1;
    mem[2] = input1.rotate_left(9) ^ input2;
    mem[3] = input2 % 16;
    let mut pc = 0usize;
    let mut steps = 0u32;
    let mut fault = 0u32;
    loop {
        if steps >= STEP_BUDGET {
            fault = 9;
            break;
        }
        steps += 1;
        let op = prog[pc & 63];
        pc = (pc + 1) & 63;
        match op {
            0 => break,
            1 => {
                if sp >= 16 {
                    fault = 1;
                    break;
                }
                stack[sp] = prog[pc & 63] as u32;
                pc = (pc + 1) & 63;
                sp += 1;
            }
            2 => {
                if sp == 0 || sp >= 16 {
                    fault = 2;
                    break;
                }
                stack[sp] = stack[sp - 1];
                sp += 1;
            }
            3 => {
                if sp < 2 {
                    fault = 3;
                    break;
                }
                stack.swap(sp - 1, sp - 2);
            }
            4 => {
                if sp < 2 {
                    fault = 4;
                    break;
                }
                let b = stack[sp - 1];
                sp -= 1;
                stack[sp - 1] = stack[sp - 1].wrapping_add(b);
            }
            5 => {
                if sp < 2 {
                    fault = 5;
                    break;
                }
                let b = stack[sp - 1];
                sp -= 1;
                stack[sp - 1] = stack[sp - 1].wrapping_mul(b);
            }
            6 => {
                if sp < 2 {
                    fault = 6;
                    break;
                }
                let b = stack[sp - 1];
                sp -= 1;
                stack[sp - 1] ^= b;
            }
            7 => {
                if sp < 2 {
                    fault = 7;
                    break;
                }
                let n = stack[sp - 1];
                sp -= 1;
                stack[sp - 1] = stack[sp - 1].rotate_left(n & 31);
            }
            8 => {
                if sp < 2 {
                    fault = 8;
                    break;
                }
                let n = stack[sp - 1];
                sp -= 1;
                stack[sp - 1] >>= n & 31;
            }
            9 => {
                pc = (prog[pc & 63] & 63) as usize;
            }
            10 => {
                if sp < 1 {
                    fault = 10;
                    break;
                }
                let c = stack[sp - 1];
                sp -= 1;
                let target = (prog[pc & 63] & 63) as usize;
                pc = if c == 0 { target } else { (pc + 1) & 63 };
            }
            11 => {
                if csp >= 8 {
                    fault = 11;
                    break;
                }
                let target = (prog[pc & 63] & 63) as usize;
                cstack[csp] = ((pc + 1) & 63) as u8;
                csp += 1;
                pc = target;
            }
            12 => {
                if csp == 0 {
                    fault = 12;
                    break;
                }
                csp -= 1;
                pc = cstack[csp] as usize;
            }
            13 => {
                if sp < 1 {
                    fault = 13;
                    break;
                }
                let a = stack[sp - 1];
                stack[sp - 1] = mem[(a & 15) as usize];
            }
            14 => {
                if sp < 2 {
                    fault = 14;
                    break;
                }
                let a = stack[sp - 1];
                let v = stack[sp - 2];
                sp -= 2;
                mem[(a & 15) as usize] = v;
            }
            15 => {
                if sp < 2 {
                    fault = 15;
                    break;
                }
                let b = stack[sp - 1];
                sp -= 1;
                stack[sp - 1] = stack[sp - 1].wrapping_sub(b);
            }
            16 => {
                if sp < 1 {
                    fault = 16;
                    break;
                }
                stack[sp - 1] = stack[sp - 1].wrapping_sub(1);
            }
            17 => {
                if sp < 2 || sp >= 16 {
                    fault = 17;
                    break;
                }
                stack[sp] = stack[sp - 2];
                sp += 1;
            }
            _ => {
                fault = 0xff;
                break;
            }
        }
    }
    let mut acc = fault.wrapping_mul(0x9e37_79b9)
        ^ steps
        ^ ((pc as u32) << 8)
        ^ ((sp as u32) << 16)
        ^ ((csp as u32) << 24);
    let mut i = 0usize;
    while i < 16 {
        acc = acc.rotate_left(5) ^ stack[i].wrapping_mul(i as u32 * 2 + 1);
        acc = acc.rotate_left(3) ^ mem[i];
        i += 1;
    }
    i = 0;
    while i < 8 {
        acc ^= (cstack[i] as u32) << (i * 4);
        i += 1;
    }
    acc
}
