// Bytecode interpreter with a handler table (campaign 17, program 15): the
// same 18-opcode stack VM and `.rodata` program images as `prog_stack_vm`,
// but with the VM state in a struct passed by `&mut` to opcode handlers
// dispatched through a `.rodata` table of function pointers (one
// `call_indirect` per step, the receiver pointer as the only argument),
// each handler decoding its own immediates through a shared helper and
// returning whether execution continues; the whole VM state is folded.
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

struct Vm {
    stack: [u32; 16],
    mem: [u32; 16],
    cstack: [u8; 8],
    sp: usize,
    csp: usize,
    pc: usize,
    fault: u32,
    prog: &'static [u8; 64],
}

impl Vm {
    fn imm(&mut self) -> u8 {
        let v = self.prog[self.pc & 63];
        self.pc = (self.pc + 1) & 63;
        v
    }

    fn pop(&mut self) -> Option<u32> {
        if self.sp == 0 {
            return None;
        }
        self.sp -= 1;
        Some(self.stack[self.sp])
    }

    fn push(&mut self, v: u32) -> bool {
        if self.sp >= 16 {
            return false;
        }
        self.stack[self.sp] = v;
        self.sp += 1;
        true
    }

    fn binary(&mut self, f: fn(u32, u32) -> u32) -> bool {
        match (self.pop(), self.pop()) {
            (Some(b), Some(a)) => self.push(f(a, b)),
            _ => false,
        }
    }
}

type Handler = fn(&mut Vm) -> bool;

fn op_halt(_vm: &mut Vm) -> bool {
    false
}

fn op_push(vm: &mut Vm) -> bool {
    let v = vm.imm() as u32;
    vm.push(v)
}

fn op_dup(vm: &mut Vm) -> bool {
    match vm.pop() {
        Some(v) => vm.push(v) && vm.push(v),
        None => false,
    }
}

fn op_swap(vm: &mut Vm) -> bool {
    match (vm.pop(), vm.pop()) {
        (Some(b), Some(a)) => vm.push(b) && vm.push(a),
        _ => false,
    }
}

fn op_add(vm: &mut Vm) -> bool {
    vm.binary(|a, b| a.wrapping_add(b))
}

fn op_mul(vm: &mut Vm) -> bool {
    vm.binary(|a, b| a.wrapping_mul(b))
}

fn op_xor(vm: &mut Vm) -> bool {
    vm.binary(|a, b| a ^ b)
}

fn op_rotl(vm: &mut Vm) -> bool {
    vm.binary(|a, b| a.rotate_left(b & 31))
}

fn op_shr(vm: &mut Vm) -> bool {
    vm.binary(|a, b| a >> (b & 31))
}

fn op_jmp(vm: &mut Vm) -> bool {
    let t = vm.imm();
    vm.pc = (t & 63) as usize;
    true
}

fn op_jz(vm: &mut Vm) -> bool {
    let t = vm.imm();
    match vm.pop() {
        Some(0) => {
            vm.pc = (t & 63) as usize;
            true
        }
        Some(_) => true,
        None => false,
    }
}

fn op_call(vm: &mut Vm) -> bool {
    let t = vm.imm();
    if vm.csp >= 8 {
        return false;
    }
    vm.cstack[vm.csp] = vm.pc as u8;
    vm.csp += 1;
    vm.pc = (t & 63) as usize;
    true
}

fn op_ret(vm: &mut Vm) -> bool {
    if vm.csp == 0 {
        return false;
    }
    vm.csp -= 1;
    vm.pc = vm.cstack[vm.csp] as usize;
    true
}

fn op_load(vm: &mut Vm) -> bool {
    match vm.pop() {
        Some(a) => {
            let v = vm.mem[(a & 15) as usize];
            vm.push(v)
        }
        None => false,
    }
}

fn op_store(vm: &mut Vm) -> bool {
    match (vm.pop(), vm.pop()) {
        (Some(a), Some(v)) => {
            vm.mem[(a & 15) as usize] = v;
            true
        }
        _ => false,
    }
}

fn op_sub(vm: &mut Vm) -> bool {
    vm.binary(|a, b| a.wrapping_sub(b))
}

fn op_dec(vm: &mut Vm) -> bool {
    match vm.pop() {
        Some(v) => vm.push(v.wrapping_sub(1)),
        None => false,
    }
}

fn op_over(vm: &mut Vm) -> bool {
    if vm.sp < 2 {
        return false;
    }
    let v = vm.stack[vm.sp - 2];
    vm.push(v)
}

static HANDLERS: [Handler; 18] = [
    op_halt, op_push, op_dup, op_swap, op_add, op_mul, op_xor, op_rotl, op_shr, op_jmp, op_jz,
    op_call, op_ret, op_load, op_store, op_sub, op_dec, op_over,
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut vm = Vm {
        stack: [0u32; 16],
        mem: [0u32; 16],
        cstack: [0u8; 8],
        sp: 0,
        csp: 0,
        pc: 0,
        fault: 0,
        prog: &PROGS[(input2 & 3) as usize],
    };
    vm.push(input2);
    vm.push(input1);
    vm.mem[0] = (input2 >> 4) % 48;
    vm.mem[1] = input1 | 1;
    vm.mem[2] = input2.rotate_left(5) ^ input1;
    vm.mem[3] = input1 % 16;
    let mut steps = 0u32;
    loop {
        if steps >= 640 {
            vm.fault = 9;
            break;
        }
        steps += 1;
        let op = vm.imm();
        if op as usize >= HANDLERS.len() {
            vm.fault = 0xff;
            break;
        }
        let handler = HANDLERS[op as usize];
        if !handler(&mut vm) {
            vm.fault = if op == 0 { 0 } else { op as u32 };
            break;
        }
    }
    let mut acc = vm.fault.wrapping_mul(0x9e37_79b9)
        ^ steps
        ^ ((vm.pc as u32) << 8)
        ^ ((vm.sp as u32) << 16)
        ^ ((vm.csp as u32) << 24);
    let mut i = 0usize;
    while i < 16 {
        acc = acc.rotate_left(5) ^ vm.stack[i].wrapping_mul(i as u32 * 2 + 1);
        acc = acc.rotate_left(3) ^ vm.mem[i];
        i += 1;
    }
    i = 0;
    while i < 8 {
        acc ^= (vm.cstack[i] as u32) << (i * 4);
        i += 1;
    }
    acc
}
