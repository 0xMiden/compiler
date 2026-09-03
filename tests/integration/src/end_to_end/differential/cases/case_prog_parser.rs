// Tokenizer + parser + evaluator (campaign 17, program 9): an expression
// text in `.rodata` (eight `;`-separated infix expressions with numbers, a
// variable, parentheses and eight operators of five precedence levels) is
// tokenized from an input-selected expression, converted to postfix by a
// shunting-yard with an operator stack, built into an AST in a stack
// arena (index-linked nodes), and evaluated twice — in arena order and by
// an explicit-stack post-order walk — with zero-guarded i32 division and
// remainder; results, node / token counts and hashes of the postfix buffer
// and the arena are folded.
static TEXT: &[u8] = b"(12+34)*5-6/(3+7)*(8-2)+x;x*x-3*x+7;(x+1)*(x-1)/(x%7+1);100/(x-x)+1;((((1+2)*3)+4)*5)-x*2;7*8+9*10-11*12+x/3;(x|5)&(x^3)-x%(x/2+1);2*(3+x)*(4-x)*(5+x)|x&255";

const T_NUM: u32 = 1 << 24;
const T_VAR: u32 = 2 << 24;
const T_OP: u32 = 3 << 24;

fn prec(op: u8) -> u32 {
    match op {
        b'*' | b'/' | b'%' => 5,
        b'+' | b'-' => 4,
        b'&' => 3,
        b'^' => 2,
        b'|' => 1,
        _ => 0,
    }
}

fn apply(op: u8, a: i32, b: i32) -> i32 {
    match op {
        b'+' => a.wrapping_add(b),
        b'-' => a.wrapping_sub(b),
        b'*' => a.wrapping_mul(b),
        b'/' => {
            if b == 0 {
                0
            } else {
                a.wrapping_div(b)
            }
        }
        b'%' => {
            if b == 0 {
                0
            } else {
                a.wrapping_rem(b)
            }
        }
        b'&' => a & b,
        b'^' => a ^ b,
        _ => a | b,
    }
}

// Tokenizes one expression into postfix (shunting-yard). Returns the
// postfix token count.
fn to_postfix(start: usize, out: &mut [u32; 64]) -> usize {
    let mut ops = [0u8; 32];
    let mut nops = 0usize;
    let mut n = 0usize;
    let mut i = start;
    while i < TEXT.len() && n < 60 {
        let c = TEXT[i];
        if c == b';' {
            break;
        }
        if c.is_ascii_digit() {
            let mut v = 0u32;
            while i < TEXT.len() && TEXT[i].is_ascii_digit() {
                v = v.wrapping_mul(10).wrapping_add((TEXT[i] - b'0') as u32);
                i += 1;
            }
            out[n] = T_NUM | (v & 0xff_ffff);
            n += 1;
            continue;
        }
        i += 1;
        if c == b'x' {
            out[n] = T_VAR;
            n += 1;
        } else if c == b'(' {
            if nops < 32 {
                ops[nops] = c;
                nops += 1;
            }
        } else if c == b')' {
            while nops > 0 && ops[nops - 1] != b'(' {
                nops -= 1;
                out[n] = T_OP | ops[nops] as u32;
                n += 1;
            }
            if nops > 0 {
                nops -= 1;
            }
        } else if prec(c) > 0 {
            while nops > 0 && prec(ops[nops - 1]) >= prec(c) && n < 60 {
                nops -= 1;
                out[n] = T_OP | ops[nops] as u32;
                n += 1;
            }
            if nops < 32 {
                ops[nops] = c;
                nops += 1;
            }
        }
    }
    while nops > 0 && n < 64 {
        nops -= 1;
        if ops[nops] != b'(' {
            out[n] = T_OP | ops[nops] as u32;
            n += 1;
        }
    }
    n
}

// Arena node: [kind, left, right, value/op].
type Node = [i32; 4];

// Builds the AST from postfix; returns (root index, node count).
fn build(post: &[u32; 64], n: usize, arena: &mut [Node; 64]) -> (usize, usize) {
    let mut stack = [0usize; 32];
    let mut sp = 0usize;
    let mut count = 0usize;
    let mut i = 0usize;
    while i < n && count < 64 {
        let t = post[i];
        let kind = t >> 24;
        if kind == 3 {
            if sp < 2 {
                break;
            }
            let r = stack[sp - 1];
            let l = stack[sp - 2];
            sp -= 2;
            arena[count] = [3, l as i32, r as i32, (t & 0xff) as i32];
        } else {
            arena[count] = [kind as i32, -1, -1, (t & 0xff_ffff) as i32];
        }
        if sp < 32 {
            stack[sp] = count;
            sp += 1;
        }
        count += 1;
        i += 1;
    }
    let root = if sp > 0 { stack[sp - 1] } else { 0 };
    (root, count)
}

// Evaluates every node in arena (creation) order — children precede parents.
fn eval_linear(arena: &[Node; 64], count: usize, x: i32) -> i32 {
    let mut vals = [0i32; 64];
    let mut i = 0usize;
    while i < count {
        let nd = arena[i];
        vals[i] = match nd[0] {
            1 => nd[3],
            2 => x,
            _ => apply(nd[3] as u8, vals[nd[1] as usize], vals[nd[2] as usize]),
        };
        i += 1;
    }
    if count > 0 { vals[count - 1] } else { 0 }
}

// Evaluates by an explicit-stack post-order walk from the root.
fn eval_walk(arena: &[Node; 64], root: usize, count: usize, x: i32) -> i32 {
    if count == 0 {
        return 0;
    }
    let mut todo = [0u32; 64];
    let mut vals = [0i32; 64];
    let mut tp = 0usize;
    let mut vp = 0usize;
    todo[0] = root as u32;
    tp = 1;
    let mut guard = 0u32;
    while tp > 0 && guard < 512 {
        guard += 1;
        tp -= 1;
        let e = todo[tp];
        let idx = (e & 0xffff) as usize;
        let visited = e >> 16 != 0;
        let nd = arena[idx];
        if nd[0] != 3 {
            if vp < 64 {
                vals[vp] = if nd[0] == 1 { nd[3] } else { x };
                vp += 1;
            }
        } else if !visited {
            if tp + 3 <= 64 {
                todo[tp] = e | 1 << 16;
                todo[tp + 1] = nd[2] as u32;
                todo[tp + 2] = nd[1] as u32;
                tp += 3;
            }
        } else if vp >= 2 {
            let b = vals[vp - 1];
            let a = vals[vp - 2];
            vp -= 2;
            vals[vp] = apply(nd[3] as u8, a, b);
            vp += 1;
        }
    }
    if vp > 0 { vals[vp - 1] } else { 0 }
}

fn expr_start(k: u32) -> usize {
    let mut i = 0usize;
    let mut seen = 0u32;
    while i < TEXT.len() && seen < k {
        if TEXT[i] == b';' {
            seen += 1;
        }
        i += 1;
    }
    i
}

fn run(k: u32, x: i32) -> (i32, i32, u32) {
    let mut post = [0u32; 64];
    let n = to_postfix(expr_start(k), &mut post);
    let mut arena = [[0i32; 4]; 64];
    let (root, count) = build(&post, n, &mut arena);
    let v1 = eval_linear(&arena, count, x);
    let v2 = eval_walk(&arena, root, count, x);
    let mut h = (n as u32) << 8 ^ (count as u32) ^ (root as u32) << 16;
    let mut i = 0usize;
    while i < n {
        h = h.rotate_left(3) ^ post[i];
        i += 1;
    }
    i = 0;
    while i < count {
        h = h.rotate_left(5)
            ^ (arena[i][0] as u32)
            ^ (arena[i][3] as u32).wrapping_mul(0x9e37_79b9)
            ^ (arena[i][1] as u32) << 8
            ^ (arena[i][2] as u32) << 16;
        i += 1;
    }
    (v1, v2, h)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let (a1, a2, h1) = run(input1 % 8, input2 as i32);
    let (b1, b2, h2) = run((input1 >> 3) % 8, input1 as i32);
    let same = ((a1 == a2) as u32) | ((b1 == b2) as u32) << 1;
    (a1 as u32).rotate_left(7) ^ (b1 as u32).rotate_left(13) ^ h1 ^ h2.rotate_left(19) ^ same << 30
}
